//! Parity tests: Repose scene backend vs the tiny-skia software reference.
//!
//! Each test renders the same script through both paths:
//!
//! - reference: `reassarus-renderer`'s `SoftwarePipeline` + `SoftwareBackend`
//!   (tiny-skia) → RGBA; asserts the frame actually covers pixels, so the
//!   scene comparison below is anchored to a non-empty render.
//! - new: `backends::repose::layers_to_scene` → structural assertions on
//!   the emitted `SceneNode`s (the six parity areas: outlines/shadows/blur,
//!   clip/iclip, rotations/shear/scale, karaoke, vector drawings, animated
//!   transforms).
//!
//! Needs the `repose-backend` feature (plus `software-backend` for the
//! reference side) and rustc 1.98+. No GPU is needed: scene emission is
//! pure CPU.
#![cfg(all(feature = "repose-backend", feature = "software-backend"))]

use reassarus_core::parser::Script;
use reassarus_renderer::backends::repose::{covered_pixels, layers_to_scene};
use reassarus_renderer::backends::BackendType;
use reassarus_renderer::pipeline::{IntermediateLayer, Pipeline, SoftwarePipeline};
use reassarus_renderer::renderer::{EventSelector, RenderContext, Renderer};
use repose_core::SceneNode;

const HEAD: &str = "[Script Info]\nPlayResX: 1280\nPlayResY: 720\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Default,Arial,64,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,5,30,30,30,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n";

/// Run the software pipeline for one dialogue line and return the layers.
fn layers_at(time_cs: u32, dialogue_text: &str) -> Vec<IntermediateLayer> {
    let script_text =
        format!("{HEAD}Dialogue: 0,0:00:00.00,0:00:10.00,Default,,0,0,0,,{dialogue_text}\n");
    // The borrow checker needs the owned string alive while layers borrow it.
    let script = Box::leak(Box::new(script_text));
    let script = Box::leak(Box::new(Script::parse(script).expect("parse")));
    let ctx = RenderContext::new(1280, 720);
    let mut selector = EventSelector::new();
    let active = selector
        .select_active(script, time_cs)
        .expect("select active");
    let mut pipeline = SoftwarePipeline::new();
    pipeline.prepare_script(script, None).expect("prepare");
    pipeline
        .process_events(&active.events, time_cs, &ctx)
        .expect("process")
}

/// Reference RGBA render; asserts the reference itself covers pixels.
fn reference_covers(dialogue_text: &str, time_cs: u32) -> u64 {
    let script_text =
        format!("{HEAD}Dialogue: 0,0:00:00.00,0:00:10.00,Default,,0,0,0,,{dialogue_text}\n");
    let script = Script::parse(&script_text).expect("parse");
    let ctx = RenderContext::new(1280, 720);
    let mut renderer = Renderer::new(BackendType::Software, ctx).expect("renderer");
    let frame = renderer.render_frame(&script, time_cs).expect("render");
    covered_pixels(frame.data())
}

fn text_nodes(nodes: &[SceneNode]) -> Vec<&SceneNode> {
    nodes
        .iter()
        .filter(|n| matches!(n, SceneNode::Text { .. }))
        .collect()
}

#[test]
fn plain_fill_emits_single_text_node() {
    let n = reference_covers("Hello", 200);
    assert!(n > 0, "reference must cover pixels, got {n}");
    let layers = layers_at(200, "Hello");
    let built = layers_to_scene(&layers, 1280, 720);
    assert_eq!(built.skipped_raster, 0);
    let texts = text_nodes(&built.scene.nodes);
    assert_eq!(texts.len(), 1, "plain fill: one Text node");
}

#[test]
fn outline_emits_stroke_under_fill() {
    let dialogue = r"{\bord2\3c&H0000FF&}Hello";
    let n = reference_covers(dialogue, 200);
    assert!(n > 0, "reference must cover pixels, got {n}");
    let layers = layers_at(200, dialogue);
    let built = layers_to_scene(&layers, 1280, 720);
    let texts = text_nodes(&built.scene.nodes);
    assert_eq!(texts.len(), 2, "outline: stroke + fill Text nodes");
    let is_stroke = texts.iter().any(|t| match t {
        SceneNode::Text { extra_style, .. } => {
            matches!(
                extra_style.draw_style,
                repose_core::DrawStyle::Stroke { .. }
            )
        }
        _ => false,
    });
    assert!(is_stroke, "one node must use DrawStyle::Stroke");
}

#[test]
fn shadow_emits_offset_pass() {
    let dialogue = r"{\shad2}Hello";
    let n = reference_covers(dialogue, 200);
    assert!(n > 0, "reference must cover pixels, got {n}");
    let layers = layers_at(200, dialogue);
    let built = layers_to_scene(&layers, 1280, 720);
    let texts = text_nodes(&built.scene.nodes);
    assert_eq!(texts.len(), 2, "shadow: offset pass + main Text nodes");
}

#[test]
fn blur_wraps_run_in_layer() {
    let dialogue = r"{\blur3}Hello";
    let n = reference_covers(dialogue, 200);
    assert!(n > 0, "reference must cover pixels, got {n}");
    let layers = layers_at(200, dialogue);
    let built = layers_to_scene(&layers, 1280, 720);
    let has_layer = built.scene.nodes.iter().any(|node| match node {
        SceneNode::BeginLayer { blur_radius_x, .. } => blur_radius_x.0 > 0.0,
        _ => false,
    });
    assert!(has_layer, "blur must emit a BeginLayer with radius");
    assert!(
        built
            .scene
            .nodes
            .iter()
            .any(|node| matches!(node, SceneNode::EndLayer { .. })),
        "blur layer must be closed"
    );
}

#[test]
fn clip_emits_intersect_and_iclip_difference() {
    let layers = layers_at(200, r"{\clip(10,10,200,100)}Hello");
    let built = layers_to_scene(&layers, 1280, 720);
    let clip = built.scene.nodes.iter().find_map(|node| match node {
        SceneNode::PushClip { rect, op, .. } => Some((rect, op)),
        _ => None,
    });
    let (rect, op) = clip.expect("clip must emit PushClip");
    assert!(matches!(op, repose_core::ClipOp::Intersect));
    assert_eq!((rect.x, rect.y, rect.w, rect.h), (10.0, 10.0, 190.0, 90.0));

    let layers = layers_at(200, r"{\iclip(10,10,200,100)}Hello");
    let built = layers_to_scene(&layers, 1280, 720);
    let op = built.scene.nodes.iter().find_map(|node| match node {
        SceneNode::PushClip { op, .. } => Some(op),
        _ => None,
    });
    assert!(
        matches!(op, Some(repose_core::ClipOp::Difference)),
        r"\iclip must emit Difference"
    );
}

#[test]
fn rotation_emits_transform() {
    let dialogue = r"{\frz45}Hello";
    let n = reference_covers(dialogue, 200);
    assert!(n > 0, "reference must cover pixels, got {n}");
    let layers = layers_at(200, dialogue);
    let built = layers_to_scene(&layers, 1280, 720);
    let rotate = built.scene.nodes.iter().find_map(|node| match node {
        SceneNode::PushTransform { transform } => Some(transform.rotate),
        _ => None,
    });
    let rotate = rotate.expect("rotation must emit PushTransform");
    assert!(
        (rotate.abs() - 45f32.to_radians()).abs() < 1e-4,
        "rotate must be ±45°, got {rotate}"
    );
}

#[test]
fn shear_emits_transform() {
    let layers = layers_at(200, r"{\fax2}Hello");
    let built = layers_to_scene(&layers, 1280, 720);
    let shear = built.scene.nodes.iter().find_map(|node| match node {
        SceneNode::PushTransform { transform } => Some((transform.shear_x, transform.shear_y)),
        _ => None,
    });
    let (shear_x, shear_y) = shear.expect("shear must emit PushTransform");
    assert!(
        (shear_x - 2.0).abs() < 1e-4 && shear_y.abs() < 1e-4,
        "\\fax2 must land in shear_x, got ({shear_x}, {shear_y})"
    );
}

#[test]
fn karaoke_sweep_emits_progress_clip() {
    let dialogue = r"{\k100}Ka{\k100}ra";
    // Mid second syllable: first fully sung, second partially.
    let n = reference_covers(dialogue, 150);
    assert!(n > 0, "reference must cover pixels, got {n}");
    let layers = layers_at(150, dialogue);
    let built = layers_to_scene(&layers, 1280, 720);
    let clips: Vec<_> = built
        .scene
        .nodes
        .iter()
        .filter(|node| matches!(node, SceneNode::PushClip { .. }))
        .collect();
    assert!(
        !clips.is_empty(),
        "karaoke sweep must emit at least one progress PushClip"
    );
    assert!(
        text_nodes(&built.scene.nodes).len() >= 2,
        "karaoke needs sung + unsung Text nodes"
    );
}

#[test]
fn vector_drawing_tessellates_to_mesh() {
    let dialogue = r"{\p1}m 0 0 l 60 0 l 60 60 l 0 60{\p0}";
    let n = reference_covers(dialogue, 200);
    assert!(n > 0, "reference must cover pixels, got {n}");
    let layers = layers_at(200, dialogue);
    let built = layers_to_scene(&layers, 1280, 720);
    assert_eq!(built.skipped_tess, 0);
    let meshes: Vec<_> = built
        .scene
        .nodes
        .iter()
        .filter(|node| matches!(node, SceneNode::VectorMesh { .. }))
        .collect();
    assert_eq!(meshes.len(), 1, "drawing must emit one VectorMesh");
    if let SceneNode::VectorMesh { mesh, .. } = meshes[0] {
        assert!(!mesh.indices.is_empty(), "mesh must have triangles");
        assert!(!mesh.vertices.is_empty(), "mesh must have vertices");
    }
}

#[test]
fn animated_move_stays_renderable() {
    let dialogue = r"{\move(10,10,400,300,0,1000)}Hello";
    // Mid-flight at 5s.
    let n = reference_covers(dialogue, 500);
    assert!(n > 0, "reference must cover pixels, got {n}");
    let layers = layers_at(500, dialogue);
    let built = layers_to_scene(&layers, 1280, 720);
    assert!(
        !built.scene.nodes.is_empty(),
        "animated transform must still emit a scene"
    );
    assert!(!text_nodes(&built.scene.nodes).is_empty());
}
