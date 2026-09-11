# reassarus-renderer

> **⚠️ WORK IN PROGRESS** - This renderer is currently under active development. While the software backend is functional, some features may be incomplete and the API is subject to change.

High-performance ASS (Advanced SubStation Alpha) subtitle renderer with modular backend support.

## Features

- **Two Rendering Backends**
  - Software (CPU) rendering with tiny-skia (RECOMMENDED - fully implemented;
    also the correctness reference for the GPU path)
  - Repose (GPU) scene adapter: composes subtitles with the rest of a Repose
    UI via `repose-core` + `repose-render-wgpu` offscreen readback
  - Automatic backend selection (prefers Repose when compiled in, falls back to Software)

- **Complete ASS/SSA Support**
  - All ASS v4+ tags and formatting
  - Style inheritance and resolution
  - Complex positioning (pos, move, org)
  - Animation support (\t tags)
  - Karaoke effects
  - Drawing commands (\p)
  - Clipping (\clip, \iclip)

- **Advanced Features**
  - Collision detection and resolution
  - Incremental rendering for performance
  - Plugin system for custom effects
  - Comprehensive caching system
  - Zero-copy design where possible

## Usage

```rust
use reassarus_renderer::{Renderer, RenderContext};
use reassarus_core::parser::Script;

// Parse ASS script
let script = Script::parse(script_text)?;

// Create rendering context
let context = RenderContext::new(1920, 1080);

// Create renderer with automatic backend selection
let mut renderer = Renderer::with_auto_backend(context)?;

// Render frame at specific time (in centiseconds)
let frame = renderer.render_frame(&script, 500)?;

// Access pixel data
let pixels = frame.data(); // RGBA8 format
```

## Backends

### Software Backend (RECOMMENDED)
Pure CPU rendering using tiny-skia. **Fully implemented and production-ready.** Works everywhere, no GPU dependencies.

```rust
use reassarus_renderer::{Renderer, RenderContext, BackendType};

// Recommended: Use Software backend explicitly
let mut renderer = Renderer::new(BackendType::Software, context)?;
```

### Repose Backend (GPU scene adapter)
Emits the shared pipeline layers as a Repose `Scene` so subtitles compose
with the rest of a Repose UI, resolved to pixels through
`repose-render-wgpu` offscreen readback:

```rust
use reassarus_renderer::{Renderer, RenderContext, BackendType};

// GPU path: needs a WGPU adapter at render time
// (mesa-vulkan-drivers suffices headless); fails loudly without one.
let mut renderer = Renderer::new(BackendType::Repose, context)?;
```

The pure-CPU `backends::repose::layers_to_scene` conversion (no GPU) is what
the structural parity tests exercise (`tests/repose_parity.rs`). Current
state vs the software reference: perspective `\frx`/`\fry` uses libass's
exact 3D rotation about the `\org` pivot (differentially fit against libass
0.17.5, ≤2px), scene rects use shaping-measured bounds, and `\be` wraps
just the outline stroke while full `\blur` wraps the whole run — see
`backends::repose` docs for the full mapping.

For production use, we strongly recommend the Software backend which has full feature support and has been thoroughly tested.

## Collision Detection

Automatic collision detection prevents subtitle overlap:

```rust
use reassarus_renderer::collision::{CollisionResolver, PositionedEvent, BoundingBox};

let mut resolver = CollisionResolver::new(1920.0, 1080.0);

// Add fixed event
resolver.add_fixed(event1);

// Find non-colliding position for new event
let new_position = resolver.find_position(event2);
```

## Animation System

Support for ASS animation tags (\t):

```rust
use reassarus_renderer::animation::{AnimationController, AnimationTiming, AnimatedValue};

let mut controller = AnimationController::new();

// Add animation track
let timing = AnimationTiming::new(0, 100, 1.0);
let track = AnimationTrack::new(
    "font_size".to_string(),
    timing,
    AnimatedValue::Float { from: 20.0, to: 40.0 },
    AnimationInterpolation::Linear,
);
controller.add_track(track);

// Evaluate at specific time
let state = controller.evaluate(50);
```

## Performance

Optimized for high performance:
- Target: <5ms per 4K frame
- SIMD acceleration (when enabled)
- Incremental rendering support
- Extensive caching
- Parallel processing with rayon

## Feature Flags

- `default`: analysis integration, Repose backend, backend probing, SIMD,
  image export, serde
- `minimal`: `nostd`-compatible core (`nostd` + analysis integration)
- `full`: everything below
- `software-backend`: CPU rendering support
- `repose-backend`: GPU scene adapter (`repose-core`, `repose-render-wgpu`)
- `backend-probing` / `backend-metrics`: backend selection helpers / metrics
- `simd`: SIMD acceleration
- `arena`: Arena allocator for reduced allocations
- `analysis-integration`: Integration with reassarus-core analysis
- `image`: image export support
- `serde`: Serialization support
- `unicode-wrap`: Unicode line-break support
- `nostd`: No-std support (software backend only; the Repose backend needs
  std + GPU)
- `libass-compare`: dev-only native-libass A/B comparison

## Benchmarks

Run benchmarks with:
```bash
cargo bench --package reassarus-renderer --features benches
```

Performance targets (vs libass):
- Simple subtitles: 2-3x faster
- Complex effects: 1.5-2x faster
- 4K rendering: <5ms per frame
- Memory usage: ~1.1x input size

## Testing

```bash
cargo test --package reassarus-renderer --all-features
```

## License

MPL-2.0