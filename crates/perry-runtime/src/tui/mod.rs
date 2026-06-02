//! Native TUI engine for Perry — issue #358.
//!
//! Architectural pattern (from the issue):
//!
//! ```text
//! TS (declarative) → HIR → Codegen → calls into perry-runtime::tui (Rust) → terminal
//!                                        ├─ cell grid + dirty tracking
//!                                        ├─ double-buffered renderer
//!                                        └─ ANSI emitter (minimal escape sequences)
//! ```
//!
//! v0.1 surface (Phase 1):
//!
//! - `Box(opts?, children?)` — vertical-stack container (real flexbox lands in Phase 3 with Taffy)
//! - `Text(content)` — single-line text node
//! - `render(root)` — paints one frame to stdout
//!
//! Cell grid is a packed `Vec<Cell>` (no per-cell allocation per frame).
//! Double buffer: render to back, diff against front, emit minimal ANSI
//! to reconcile changed cells. The diff is unconditional (every render
//! call emits only what changed since the last call), so even Phase 1's
//! one-shot `render()` already pays the architecture's cost — no
//! retrofit later.
//!
//! Lives inside perry-runtime (rather than a sibling perry-tui crate
//! the issue originally described) so the FFI symbols `js_perry_tui_*`
//! are bundled into libperry_runtime.a unconditionally — no separate
//! linker flag, no auto-optimize feature gate, just `import { Box,
//! Text, render } from "perry/tui"` and it works. Architecturally
//! still one logical module.
//!
//! Interactive loop, hooks, and Taffy flexbox layer on top of this in
//! Phases 2 / 3 / 4.

pub mod cell;
pub mod color;
pub mod ffi;
pub mod hooks;
pub mod input;
pub mod layout;
pub mod render;
pub mod run;
pub mod state;
pub mod style;
pub mod tree;
