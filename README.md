# ASS-RS

[![Crates.io](https://img.shields.io/crates/v/reassarus-core.svg)](https://crates.io/crates/reassarus-core)
[![Documentation](https://docs.rs/reassarus-core/badge.svg)](https://docs.rs/reassarus-core)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![CI](https://github.com/mlm-games/reassarus/workflows/CI/badge.svg)](https://github.com/mlm-games/reassarus/actions)

A modular, high-performance Rust implementation of the ASS (Advanced SubStation Alpha) subtitle format.

## 🚀 Key Advantages

- **Memory Safety**: 100% safe Rust with zero unsafe code
- **Modularity**: Trait-based plugin system vs. monolithic C codebase
- **Performance**: <5ms parsing with zero-copy spans, SIMD optimizations
- **Thread Safety**: Immutable `Script` design with `Send + Sync`
- **Extensibility**: Runtime plugin registry for custom tags/sections
- **Modern Standards**: Growing libass 0.17.x parity (differentially tested, e.g. ≤2px perspective fit), Unicode wrapping via UAX #14
- **Cross-Platform**: Pure Rust with nostd-compatible core/editor paths for embedded targets

## 📖 Specifications

This implementation adheres to official ASS/SSA specifications:

- **[TCax ASS Specification](http://www.tcax.org/docs/ass-specs.htm)** - Official ASS format documentation
- **[Aegisub ASS Tags](https://aegisub.org/docs/latest/ass_tags/)** -  Tag reference
- **[libass ASS Guide](https://github.com/libass/libass/wiki/ASS-File-Format-Guide)** - Extensions and implementation notes
- **[SSA v4.00 Original](http://www.eswat.demon.co.uk/)** - Legacy SSA compatibility

## 🏗️ Architecture

The ASS-RS ecosystem consists of modular, interoperable crates:

```
┌─────────────┐    ┌──────────────┐    ┌─────────────┐
│  reassarus-core   │────│ reassarus-renderer │    │ reassarus-editor  │
│   (parser)  │    │  (rendering) │    │ (editing)   │
└─────────────┘    └──────────────┘    └─────────────┘
```

- **`reassarus-core`**: Zero-copy parsing, analysis, and AST manipulation — *available, stable*
- **`reassarus-editor`**: Interactive editing APIs with incremental updates — *available, stable*
- **`reassarus-renderer`**: Software (tiny-skia + scanline/coverage cache) and Repose GPU scene-adapter backends — *software backend is the correctness reference with pixel-level tests; Repose adapter has structural parity coverage (see below), pixel comparison is structural-similarity (different shaping stacks, needs a WGPU adapter at render time)*
- **`ass-cli`**: Command-line tools for processing and conversion — *planned (crate does not exist yet)*
- **`ass-benchmarks`**: Performance testing and libass comparisons — *planned as a standalone crate (per-crate benches and a dev-only native-libass A/B example exist today)*

### Renderer parity status

- **Software backend (reference)**: pixel-level regression tests in `crates/reassarus-renderer/tests/software_render.rs` — inline colors, `\frz` direction/geometry, `\frx`/`\fry` staying on-screen, `\fax`/`\fay` shear, `\bord`/`\shad`, `\blur`/`\be` softening fill+outline+shadow together, `\clip`/`\iclip` partitioning, `\r` reset, complex `\fade` holds, `\k`/`\kf` karaoke colors, `\p` drawings, BorderStyle 3 opaque boxes, `\org` pivots, `\t` growth from base size, multi-line spacing/centering, auto-wrap, collision stacking, static-frame cache consistency, ms/cs clock agreement, smooth `\move` interpolation.
- **Repose GPU adapter** (`backends::repose::layers_to_scene`, pure CPU, no GPU needed): structural parity tests in `tests/repose_parity.rs` — outline stroke-under-fill, shadow offset pass at full ASS depth, whole-run `\blur` vs outline-only `\be` layers, `\clip`/`\iclip` ops, `\frz` rotation, `\fax`/`\fay` shear, ASS-percent scale normalization, measured shaping bounds forwarded to scene rects, resolved font family, `\k` flip vs `\K` sweep clips, vector fill+stroke mesh passes, animated `\move`, transparency culling, distant `\org` pivots, and perspective `\frx`/`\fry` (libass 0.17.5 rotation, ≤2px) with `\org` as a fixed point of the projective map.
- **Pixel-level Repose vs software** (`tests/repose_pixel_parity.rs`): structural similarity (coverage, ink mass ballpark, line centres) — not bit-identity, since the stacks shape text differently. Skips loudly without a WGPU adapter. Exact-pixel A/B against native libass lives in `examples/libass_ffi_compare.rs` and requires the dev-only `libass-compare` feature plus an installed libass.
- **Backend selection**: `BackendType::Auto` (and `Renderer::with_auto_backend`) prefers **Repose** when compiled in, falling back to **Software**; select `Software` explicitly for headless-without-GPU environments.

## ⚡ Performance Targets

- **Parsing**: <5ms for typical scripts (1KB-10KB)
- **Incremental Updates**: <2ms for single-event modifications
- **Memory Usage**: ~1.1x input size via zero-copy spans
- **SIMD Acceleration**: 20-30% faster with portable SIMD
- **Streaming**: <10ms/MB for chunked inputs


## 🎯 Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
reassarus-core = "0.1.2"
```

Basic usage:

```rust
use reassarus_core::{Script, ScriptAnalysis, Section};

let script_text = r#"
[Script Info]
Title: Example Karaoke
ScriptType: v4.00+

[V4+ Styles]
Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding
Style: Default,Arial,20,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,0,2,10,10,10,1

[Events]
Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text
Dialogue: 0,0:00:00.00,0:00:05.00,Default,,0,0,0,,{\k50}Ka{\k50}ra{\k100}oke
"#;

// Zero-copy parsing
let script = Script::parse(script_text)?;

// Analysis and linting
let analysis = ScriptAnalysis::analyze(&script)?;
for issue in analysis.lint_issues() {
    println!("Warning: {}", issue);
}

// Access parsed data with zero-copy spans
for section in script.sections() {
    if let Section::Events(events) = section {
        for event in events.iter().filter(|e| e.is_dialogue()) {
            println!("Text: {}", event.text);
            println!("Start (cs): {}", event.start_time_cs()?);
        }
    }
}

// For complex nested transforms, use the override-block parser
use reassarus_core::analysis::events::parse_override_block;

let mut tags = Vec::new();
let mut diagnostics = Vec::new();
let complex_transform = r"\t(0,1000,\fs50\1c&HFF0000&)";
parse_override_block(complex_transform, 0, &mut tags, &mut diagnostics);
```

## 🔧 Features

Enable features as needed:

```toml
[dependencies]
reassarus-core = { version = "0.1", features = ["simd", "arena", "serde"] }
```

Two flavors select feature sets (mirrored by `reassarus-editor`):

- **`full`** (default): `std` + `analysis` + `plugins` + `stream` + `simd` + `arena` + `unicode-wrap` + `serde`
- **`minimal`**: `nostd`-compatible base — `nostd` + `analysis` + `plugins` + `stream`

Granular features:

- **`std` / `nostd`**: Standard library vs embedded-friendly builds (mutually exclusive in practice)
- **`analysis`**: Deep analysis and linting capabilities
- **`plugins`**: Extension registry for custom handlers
- **`stream`**: Chunked processing for large files
- **`unicode-wrap`**: Unicode line-break support (UAX #14, libass 0.17.4+ style)
- **`simd` / `simd-full`**: SIMD-accelerated parsing (extended UUencode/hex path in `simd-full`)
- **`arena`**: Arena allocation for reduced memory overhead
- **`serde`**: Serialization support (`alloc`-only, no `std` required)
- **`benches`**: Benchmarking infrastructure

## 🧪 Testing and Benchmarks

Run the full test suite:

```bash
# Unit and integration tests (per crate; features differ per crate)
cargo test -p reassarus-core --all-features
cargo test -p reassarus-editor --all-features
cargo test -p reassarus-renderer --all-features

# Renderer parity suites (need the matching backend features)
cargo test -p reassarus-renderer --features software-backend,analysis-integration
cargo test -p reassarus-renderer --features repose-backend,software-backend

# Performance benchmarks (per-crate `benches` feature)
cargo bench -p reassarus-core --features benches
cargo bench -p reassarus-renderer --features benches

# Exact-pixel A/B against native libass (dev-only: needs installed libass)
cargo run -p reassarus-renderer --example libass_ffi_compare --features software-backend,image,analysis-integration,libass-compare
```

### Development Setup

```bash
# Clone repository
git clone https://github.com/mlm-games/reassarus.git
cd reassarus

# Run tests
cargo test --all-features

# Check code quality
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings

# Run benchmarks
cargo bench
```

### Code Quality Standards

- **No unsafe code** - 100% memory safe Rust
- **>90% test coverage** - Comprehensive testing required
- **Strict linting** - All Clippy warnings must be resolved
- **Performance validation** - No >10% regressions allowed
- **Documentation** - All public APIs documented with examples

## 📋 Roadmap

### v0.1.0 - Core Foundation ✅
- [x] Zero-copy ASS parser
- [x] Comprehensive AST with span support
- [x] Plugin system architecture
- [x] SIMD-optimized tokenization
- [x] Full spec compliance testing

### v0.2.0 - Rendering Pipeline (In Progress)
- [x] Software rasterizer backend (tiny-skia + scanline/coverage cache; pixel-level regression suite)
- [x] Text shaping via rustybuzz
- [x] Repose GPU scene adapter (`repose-core` + `repose-render-wgpu` offscreen readback) with structural parity coverage vs the software reference
- [x] Animation timeline evaluation (`\t`, `\move`, `\fade`, karaoke) on a native millisecond clock with animated-frame cache bypass
- [ ] Bit-identical GPU pixel parity (currently structural similarity: coverage, ink-mass ballpark, line centres)
- [ ] Exact-pixel libass A/B beyond the dev-only `libass-compare` example

### v0.3.0 - Editor Integration ✅
- [x] Incremental parsing for editors (<1ms edits, <5ms re-parses)
- [x] Real-time style preview and validation
- [x] Multi-document session management  
- [x] Undo/redo with efficient deltas and arena pooling

### v1.0.0 - Production Ready
- [ ] Complete libass API parity
- [ ] Production battle-testing
- [ ] Comprehensive documentation

## 📄 License

Licensed under the [MIT license](LICENSE).
