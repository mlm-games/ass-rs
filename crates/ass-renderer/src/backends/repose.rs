//! GPU backend emitting a Repose [`Scene`](repose_core::Scene).
//!
//! The `tiny-skia` software backend stays the parity reference: it owns
//! layout, shaping and effect evaluation. This backend only converts the
//! resulting [`IntermediateLayer`](crate::pipeline::IntermediateLayer)s into
//! Repose scene nodes, so ASS playback composes with the rest of a Repose UI
//! without any ASS-specific code inside the framework.
//!
//! [`layers_to_scene`] is pure CPU and needs no GPU, which is what the parity
//! tests exercise. [`ReposeBackend::composite_layers`] additionally resolves
//! the scene to RGBA through `OffscreenRenderer`, so it needs a WGPU adapter
//! at render time (llvmpipe via `mesa-vulkan-drivers` is enough for CI) and
//! fails loudly without one — select `Software` explicitly for
//! headless-without-GPU environments.
//!
//! Mapping (v1):
//!
//! - `Text` fill → `SceneNode::Text`; `Bold`/`Italic`/underline/strike ride
//!   on the node itself.
//! - `Outline` → overlaid stroke `Text` (under) + fill `Text` (over), since
//!   `DrawStyle` is fill-XOR-stroke per node.
//! - `Shadow` → offset duplicate `Text` behind the main one.
//! - `Blur`/`EdgeBlur` → `BeginLayer`/`EndLayer` with a blur radius around
//!   the text (whole-run approximation of the reference's glyph blur).
//! - `Karaoke` → base `Text` in the secondary colour plus a `PushClip`
//!   window of `progress * width` carrying the sung (primary) colour.
//! - `Rotation` (`\frz`) → `PushTransform`; `Scale` folds into the same
//!   transform. `Rotation.x/y` (perspective) and `Shear` have no `PushTransform`
//!   representation and are counted in [`BuiltScene::skipped_shear`] instead
//!   of being silently dropped.
//! - `Clip` → `PushClip`/`PopClip` (`Intersect`/`Difference` for
//!   `\clip`/`\iclip`).
//! - `Vector` → tessellated `VectorMesh` (solid fill, optional stroke).
//! - `Raster` → skipped: bitmap upload needs renderer image handles, which
//!   only exist behind a live `WgpuSceneRenderer`. Counted in
//!   [`BuiltScene::skipped_raster`].
//! - `OpaqueBox` → backing `Rect` node.

use std::sync::Arc;

use repose_core::{
    BlendMode, Brush, ClipOp, Color, DrawStyle, FontStyle, FontWeight, PaintDesc, Px, Rect,
    Scene, SceneNode, StrokeCap, StrokeJoin, TextAlign, TextDecoration, TextExtraStyle,
    Transform, VectorMeshData, VectorVertex,
};

use super::{BackendFeature, BackendType, RenderBackend};
use crate::pipeline::{IntermediateLayer, Pipeline, SoftwarePipeline, TextData, TextEffect, VectorData};
use crate::renderer::RenderContext;
use crate::utils::{DirtyRegion, RenderError};

/// GPU renderer for [`IntermediateLayer`]s via a Repose [`Scene`].
///
/// Construction is cheap and needs no GPU; the WGPU adapter is acquired
/// lazily on the first [`composite_layers`](RenderBackend::composite_layers)
/// call (and re-sized from the [`RenderContext`]) so merely selecting the
/// backend never fails headless.
pub struct ReposeBackend {
    width: u32,
    height: u32,
    offscreen: Option<repose_render_wgpu::offscreen::OffscreenRenderer>,
}

impl ReposeBackend {
    /// Create a backend for frames of the context's size.
    #[must_use]
    pub fn new(context: &RenderContext) -> Self {
        Self {
            width: context.width().max(1),
            height: context.height().max(1),
            offscreen: None,
        }
    }

    /// Build the Repose scene for `layers` without needing a GPU.
    #[must_use]
    pub fn build_scene(layers: &[IntermediateLayer], width: u32, height: u32) -> BuiltScene {
        layers_to_scene(layers, width, height)
    }
}

impl RenderBackend for ReposeBackend {
    fn backend_type(&self) -> BackendType {
        BackendType::Repose
    }

    fn create_pipeline(&self) -> Result<Box<dyn Pipeline>, RenderError> {
        Ok(Box::new(SoftwarePipeline::new()))
    }

    fn composite_layers(
        &mut self,
        layers: &[IntermediateLayer],
        context: &RenderContext,
    ) -> Result<Vec<u8>, RenderError> {
        let width = context.width().max(1);
        let height = context.height().max(1);
        let scene = layers_to_scene(layers, width, height).scene;
        if self.offscreen.is_none() {
            let renderer =
                repose_render_wgpu::offscreen::OffscreenRenderer::new_blocking(width, height, 4)
                    .map_err(|e| {
                        RenderError::BackendError(format!(
                            "repose backend needs a WGPU adapter (mesa-vulkan-drivers suffices for headless/CI); render failed: {e:#}"
                        ))
                    })?;
            self.offscreen = Some(renderer);
            self.width = width;
            self.height = height;
        }
        let offscreen = self.offscreen.as_mut().expect("initialised above");
        if width != self.width || height != self.height {
            offscreen.ensure_size(width, height).map_err(|e| {
                RenderError::BackendError(format!("repose offscreen resize failed: {e:#}"))
            })?;
            self.width = width;
            self.height = height;
        }
        offscreen.render_rgba(&scene, None).map_err(|e| {
            RenderError::BackendError(format!("repose offscreen render failed: {e:#}"))
        })
    }

    fn composite_layers_incremental(
        &mut self,
        layers: &[IntermediateLayer],
        dirty_regions: &[DirtyRegion],
        previous_frame: &[u8],
        context: &RenderContext,
    ) -> Result<Vec<u8>, RenderError> {
        let _ = (dirty_regions, previous_frame);
        self.composite_layers(layers, context)
    }

    fn supports_feature(&self, feature: BackendFeature) -> bool {
        matches!(feature, BackendFeature::HardwareAcceleration)
    }
}

/// Output of [`layers_to_scene`] with loss accounting.
///
/// Anything the scene graph cannot express (raster uploads, shear,
/// perspective rotation, failed tessellation) is counted here so parity
/// tests fail loudly instead of comparing a silently degraded scene.
#[derive(Debug)]
pub struct BuiltScene {
    /// The composed scene, ready for `render_scene_to_encoder` or offscreen
    /// readback via `OffscreenRenderer`.
    pub scene: Scene,
    /// `Raster` layers skipped (need live renderer image handles).
    pub skipped_raster: usize,
    /// `Shear` effects plus `Rotation.x/y` skipped (no `PushTransform`
    /// representation for skew/perspective).
    pub skipped_shear: usize,
    /// Vector layers whose tessellation failed.
    pub skipped_tess: usize,
}

/// Convert pipeline layers into a Repose [`Scene`].
///
/// `width`/`height` are the frame size in physical pixels; the scene clears
/// to transparent so subtitles composite over video.
#[must_use]
pub fn layers_to_scene(layers: &[IntermediateLayer], width: u32, height: u32) -> BuiltScene {
    let _ = (width, height);
    let mut out = BuiltScene {
        scene: Scene {
            clear_color: Color::from_rgba(0, 0, 0, 0),
            nodes: Vec::new(),
        },
        skipped_raster: 0,
        skipped_shear: 0,
        skipped_tess: 0,
    };
    let mut layers_ctx = LayerCtx { next_layer_id: 1 };
    for layer in layers {
        match layer {
            IntermediateLayer::Raster(_) => out.skipped_raster += 1,
            IntermediateLayer::Vector(data) => {
                if !emit_vector(&mut out, data) {
                    out.skipped_tess += 1;
                }
            }
            IntermediateLayer::Text(data) => emit_text(&mut out, &mut layers_ctx, data),
        }
    }
    out
}

/// Per-scene counter for graphics-layer ids.
struct LayerCtx {
    next_layer_id: u32,
}

impl LayerCtx {
    fn alloc(&mut self) -> u32 {
        let id = self.next_layer_id;
        self.next_layer_id = self.next_layer_id.wrapping_add(1).max(1);
        id
    }
}

/// Rough text bounds: the pipeline emits pen positions, not laid-out boxes,
/// so estimate width from glyph count for clip/blur/layer rects.
fn estimate_rect(x: f32, y: f32, text: &str, font_size: f32, spacing: f32) -> Rect {
    let glyphs = text.chars().count().max(1) as f32;
    let w = (glyphs * font_size * 0.6 + spacing * glyphs).max(1.0);
    let h = (font_size * 1.4).max(1.0);
    Rect { x, y, w, h }
}

/// Bundled parameters for one `SceneNode::Text` emission.
#[derive(Clone, Copy)]
struct TextPass<'a> {
    text: &'a str,
    rect: Rect,
    color: [u8; 4],
    font_size: f32,
    spacing: f32,
    weight: FontWeight,
    style: FontStyle,
    decoration: TextDecoration,
}

/// Outline colour plus width in pixels.
type OutlineSpec = ([u8; 4], f32);

/// `\frz` degrees plus `\org` rotation centre in screen pixels.
type RotationSpec = (f32, f32, f32, Option<(f32, f32)>);

fn text_node(pass: &TextPass<'_>, draw_style: DrawStyle) -> SceneNode {
    SceneNode::Text {
        rect: pass.rect,
        text: Arc::from(pass.text),
        color: Color::from_rgba(pass.color[0], pass.color[1], pass.color[2], pass.color[3]),
        size: Px(pass.font_size.max(1.0)),
        font_family: None,
        text_align: TextAlign::Unspecified,
        font_weight: pass.weight,
        font_style: pass.style,
        text_decoration: pass.decoration,
        letter_spacing: Px(pass.spacing),
        line_height: Px((pass.font_size * 1.2).max(1.0)),
        extra_style: TextExtraStyle {
            draw_style,
            ..TextExtraStyle::default()
        },
        url: None,
        font_variation_settings: None,
    }
}

/// Emit one fill pass of `pass`, honouring the outline effect as a
/// stroke-under-fill double emit.
fn emit_fill_pass(nodes: &mut Vec<SceneNode>, pass: &TextPass<'_>, outline: Option<OutlineSpec>) {
    if let Some((outline_color, outline_width)) = outline {
        let em = (outline_width / pass.font_size.max(1.0)).clamp(0.01, 0.5);
        nodes.push(text_node(
            &TextPass {
                color: outline_color,
                decoration: TextDecoration::default(),
                ..*pass
            },
            DrawStyle::Stroke {
                width: em,
                cap: StrokeCap::Round,
                join: StrokeJoin::Round,
                miter: 4.0,
                path_effect: None,
            },
        ));
    }
    nodes.push(text_node(pass, DrawStyle::Fill));
}

/// Emit a `Text` layer with all of its effects.
fn emit_text(out: &mut BuiltScene, layers_ctx: &mut LayerCtx, data: &TextData) {
    let mut weight = FontWeight::NORMAL;
    let mut style = FontStyle::Normal;
    let mut decoration = TextDecoration::default();
    let mut outline: Option<OutlineSpec> = None;
    let mut shadow: Option<([u8; 4], f32, f32)> = None;
    let mut blur: Option<f32> = None;
    let mut karaoke: Option<(f32, [u8; 4])> = None;
    let mut rotation: Option<RotationSpec> = None;
    let mut scale: Option<(f32, f32)> = None;
    let mut clip: Option<(f32, f32, f32, f32, bool)> = None;
    let mut opaque: Option<([u8; 4], f32)> = None;

    for effect in data.effects.iter() {
        match effect {
            TextEffect::Bold => weight = FontWeight::BOLD,
            TextEffect::Italic => style = FontStyle::Italic,
            TextEffect::Underline => decoration.underline = true,
            TextEffect::Strikethrough => decoration.strikethrough = true,
            TextEffect::Outline { color, width } => outline = Some((*color, *width)),
            TextEffect::Shadow {
                color,
                x_offset,
                y_offset,
            } => shadow = Some((*color, *x_offset, *y_offset)),
            TextEffect::Blur { radius } => blur = Some(blur.unwrap_or(0.0).max(*radius)),
            TextEffect::EdgeBlur { radius } => blur = Some(blur.unwrap_or(0.0).max(*radius)),
            TextEffect::Karaoke {
                progress,
                secondary,
                ..
            } => karaoke = Some((progress.clamp(0.0, 1.0), *secondary)),
            TextEffect::Rotation { x, y, z, origin } => {
                if *x != 0.0 || *y != 0.0 {
                    out.skipped_shear += 1;
                }
                rotation = Some((*x, *y, *z, *origin));
            }
            TextEffect::Shear { .. } => out.skipped_shear += 1,
            TextEffect::Scale { x, y } => scale = Some((*x, *y)),
            TextEffect::Clip {
                x1,
                y1,
                x2,
                y2,
                inverse,
            } => clip = Some((*x1, *y1, *x2, *y2, *inverse)),
            TextEffect::OpaqueBox { color, padding } => opaque = Some((*color, *padding)),
        }
    }

    let rect = estimate_rect(data.x, data.y, &data.text, data.font_size, data.spacing);
    let base = TextPass {
        text: &data.text,
        rect,
        color: data.color,
        font_size: data.font_size,
        spacing: data.spacing,
        weight,
        style,
        decoration,
    };
    let nodes = &mut out.scene.nodes;

    if let Some((x1, y1, x2, y2, inverse)) = clip {
        nodes.push(SceneNode::PushClip {
            rect: Rect {
                x: x1,
                y: y1,
                w: (x2 - x1).max(0.0),
                h: (y2 - y1).max(0.0),
            },
            radius: [Px::ZERO; 4],
            op: if inverse {
                ClipOp::Difference
            } else {
                ClipOp::Intersect
            },
        });
    }

    let has_transform = rotation.is_some() || scale.is_some();
    if has_transform {
        let (sx, sy) = scale.unwrap_or((1.0, 1.0));
        let z_deg = rotation.map_or(0.0, |(_, _, z, _)| z);
        // ASS rotates counter-clockwise in degrees; repose takes radians.
        let rotate = -z_deg.to_radians();
        let origin = rotation.and_then(|(_, _, _, o)| o);
        let (origin_x, origin_y) = origin.map_or((0.5, 0.5), |(ox, oy)| {
            (
                ((ox - rect.x) / rect.w.max(1.0)).clamp(0.0, 1.0),
                ((oy - rect.y) / rect.h.max(1.0)).clamp(0.0, 1.0),
            )
        });
        nodes.push(SceneNode::PushTransform {
            transform: Transform {
                translate_x: 0.0,
                translate_y: 0.0,
                scale_x: sx,
                scale_y: sy,
                rotate,
                origin_x,
                origin_y,
            },
        });
    }

    // Blur wraps the whole run in an offscreen layer (approximation of the
    // reference's per-glyph blur temps).
    let layer_id = blur.filter(|r| *r > 0.0).map(|radius| {
        let id = layers_ctx.alloc();
        nodes.push(SceneNode::BeginLayer {
            rect,
            layer_id: id,
            alpha: 1.0,
            blur_radius_x: Px(radius),
            blur_radius_y: Px(radius),
            rectangle_edge: true,
        });
        id
    });

    if let Some((color, padding)) = opaque {
        nodes.push(SceneNode::Rect {
            rect: Rect {
                x: rect.x - padding,
                y: rect.y - padding,
                w: rect.w + padding * 2.0,
                h: rect.h + padding * 2.0,
            },
            brush: Brush::Solid(Color::from_rgba(color[0], color[1], color[2], color[3])),
            radius: [Px::ZERO; 4],
        });
    }

    if let Some((color, dx, dy)) = shadow {
        let shadow_pass = TextPass {
            rect: Rect {
                x: rect.x + dx,
                y: rect.y + dy,
                w: rect.w,
                h: rect.h,
            },
            color,
            decoration: TextDecoration::default(),
            ..base
        };
        emit_fill_pass(nodes, &shadow_pass, outline);
    }

    if let Some((progress, secondary)) = karaoke {
        // Unsung base in the secondary colour, then a clipped window of the
        // sung colour sweeping left to right.
        let unsung = TextPass {
            color: secondary,
            ..base
        };
        emit_fill_pass(nodes, &unsung, outline);
        if progress > 0.0 {
            nodes.push(SceneNode::PushClip {
                rect: Rect {
                    x: rect.x,
                    y: rect.y,
                    w: rect.w * progress,
                    h: rect.h,
                },
                radius: [Px::ZERO; 4],
                op: ClipOp::Intersect,
            });
            emit_fill_pass(nodes, &base, outline);
            nodes.push(SceneNode::PopClip);
        }
    } else {
        emit_fill_pass(nodes, &base, outline);
    }

    if let Some(id) = layer_id {
        nodes.push(SceneNode::EndLayer { layer_id: id });
    }
    if has_transform {
        nodes.push(SceneNode::PopTransform);
    }
    if clip.is_some() {
        nodes.push(SceneNode::PopClip);
    }
}

/// sRGB bytes → premultiplied-linear vertex colour.
fn premult_linear(color: [u8; 4]) -> [f32; 4] {
    let lin = Color::from_rgba(color[0], color[1], color[2], color[3]).to_linear();
    [
        lin[0] * lin[3],
        lin[1] * lin[3],
        lin[2] * lin[3],
        lin[3],
    ]
}

/// Tessellate a `tiny-skia` path into a solid `VectorMesh`.
///
/// Returns `false` when there is no path or tessellation fails.
fn emit_vector(out: &mut BuiltScene, data: &VectorData) -> bool {
    use lyon_path::math::Point;
    use lyon_tessellation::{
        BuffersBuilder, FillOptions, FillTessellator, FillVertex, StrokeOptions, StrokeTessellator,
    };

    let Some(path) = &data.path else {
        return false;
    };
    let mut builder = lyon_path::Path::builder();
    let mut open = false;
    for seg in path.segments() {
        match seg {
            tiny_skia::PathSegment::MoveTo(p) => {
                if open {
                    builder.end(false);
                }
                builder.begin(Point::new(p.x, p.y));
                open = true;
            }
            tiny_skia::PathSegment::LineTo(p) => {
                if !open {
                    builder.begin(Point::new(p.x, p.y));
                    open = true;
                } else {
                    builder.line_to(Point::new(p.x, p.y));
                }
            }
            tiny_skia::PathSegment::QuadTo(c, p) => {
                if !open {
                    builder.begin(Point::new(c.x, c.y));
                    open = true;
                }
                builder.quadratic_bezier_to(Point::new(c.x, c.y), Point::new(p.x, p.y));
            }
            tiny_skia::PathSegment::CubicTo(c1, c2, p) => {
                if !open {
                    builder.begin(Point::new(c1.x, c1.y));
                    open = true;
                }
                builder.cubic_bezier_to(
                    Point::new(c1.x, c1.y),
                    Point::new(c2.x, c2.y),
                    Point::new(p.x, p.y),
                );
            }
            tiny_skia::PathSegment::Close => {
                if open {
                    builder.end(true);
                    open = false;
                }
            }
        }
    }
    if open {
        builder.end(false);
    }
    let lyon_path = builder.build();

    let color = premult_linear(data.color);
    let mut buffers: lyon_tessellation::VertexBuffers<[f32; 2], u32> =
        lyon_tessellation::VertexBuffers::new();
    let ok = if let Some(stroke) = &data.stroke {
        let options = StrokeOptions::tolerance(0.5).with_line_width(stroke.width.max(0.5));
        StrokeTessellator::new()
            .tessellate(
                &lyon_path,
                &options,
                &mut BuffersBuilder::new(&mut buffers, |v: lyon_tessellation::StrokeVertex| {
                    v.position().to_array()
                }),
            )
            .is_ok()
    } else {
        FillTessellator::new()
            .tessellate(
                &lyon_path,
                &FillOptions::tolerance(0.5),
                &mut BuffersBuilder::new(&mut buffers, |v: FillVertex| {
                    v.position().to_array()
                }),
            )
            .is_ok()
    };
    if !ok || buffers.indices.is_empty() {
        return false;
    }
    let vertices: Arc<[VectorVertex]> = buffers
        .vertices
        .iter()
        .map(|pos| VectorVertex {
            pos: *pos,
            color,
            uv: [0.0, 0.0],
        })
        .collect();
    out.scene.nodes.push(SceneNode::VectorMesh {
        mesh: Arc::new(VectorMeshData {
            vertices,
            indices: buffers.indices.into(),
        }),
        transform: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        paint: PaintDesc::Solid,
        clip: None,
        blend: BlendMode::Alpha,
    });
    true
}

/// How much of the frame the reference software backend covered.
#[must_use]
pub fn covered_pixels(rgba: &[u8]) -> u64 {
    rgba.as_chunks::<4>().0.iter().filter(|px| px[3] > 0).count() as u64
}
