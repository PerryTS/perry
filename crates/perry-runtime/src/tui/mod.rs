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
pub(crate) mod handle_object;
pub mod hooks;
pub mod input;
pub mod layout;
pub mod render;
pub mod run;
pub mod state;
pub mod style;
pub mod tree;

// #340/#341 deleted `is_known_handle` from here. It answered "is this integer
// one of our three registries' ids?" for the receiver-repr ledger, by asking
// all three under their mutexes — and it could not answer correctly, because
// `tree` counts from 1, `state` from 0 and `hooks` from 1, so one integer was
// simultaneously a live widget, a live state slot and a live ref. A tui handle
// is a heap object now; the brand on its header answers the same question with
// one load and no ambiguity (`tui::handle_object::tui_handle_parts_raw`), and
// the three `contains_handle` probes went with it.
