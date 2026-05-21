//! Centralised HIR descent helpers.
//!
//! `walk_expr_children_mut` and `walk_expr_children` are the single source of
//! truth for "what are the direct sub-expressions of an `Expr` variant?"
//! Every analysis pass that needs to descend through the HIR
//! (substitute_locals, find_max_local_id, collect_local_refs_expr,
//! remap_local_ids_in_expr, …) delegates here for the boring descent and only
//! matches the variants it actually needs to act on.
//!
//! The match below is **exhaustive on purpose** — adding a new `Expr` variant
//! to `ir.rs` without listing it here is a compile error. Historically, the
//! consumers each carried their own walker with a `_ => {}` catch-all; new
//! variants like `Uint8ArrayGet` (issue #169) and SSO-related shapes (#214)
//! silently fell through and produced runtime miscompiles. Concentrating the
//! descent in one match (which the compiler enforces) closes that bug class.
//!
//! ## What this walker does (and doesn't)
//!
//! - **Visits direct `Expr` children** — `Box<Expr>`, `Vec<Expr>`, the inner
//!   `Expr` of `ArrayElement` / `CallArg`, value-position `Expr` of `Object`
//!   / `ObjectSpread` / `I18nString.params`, etc.
//! - **Visits `Param.default` exprs of `Closure`** — these are evaluated when
//!   the closure body runs and may contain any expression.
//! - **Does NOT visit the `Closure` body** (a `Vec<Stmt>`). Consumers handle
//!   closure body descent themselves because they often want different
//!   semantics there (`replace_this_in_expr` skips closures entirely;
//!   `substitute_locals` calls its companion `_in_stmts` helper).
//! - **Does NOT visit `LocalId` fields** — the consumers that care about
//!   `LocalGet(id)`, `Update.id`, `ArrayPush.array_id`, `Closure.captures`,
//!   etc. match those variants explicitly before delegating to this walker.
//!
//! ## Adding a new `Expr` variant
//!
//! 1. Add the variant to `ir.rs::Expr`.
//! 2. The match in `walk_expr_children_mut` / `walk_expr_children` will fail
//!    to compile. Add an arm that `f`s every `Expr`-bearing field. If the
//!    variant carries no `Expr` children (e.g. a new `Math.tau` constant) the
//!    arm is `=> {}` — group it with the existing leaf arm.
//! 3. **If the variant carries a `LocalId` field** (a recurring source of
//!    bug reports — see #167, #169, #212, #214), also add explicit handling
//!    to:
//!    - `perry_transform::inline::substitute_locals`
//!    - `perry_transform::inline::find_max_local_id::check_expr`
//!    - `perry_hir::analysis::collect_local_refs_expr`
//!    - `perry_hir::analysis::remap_local_ids_in_expr`

use crate::ir::*;

mod expr_mut;
mod expr_ref;

pub use expr_mut::walk_expr_children_mut;
pub use expr_ref::walk_expr_children;
