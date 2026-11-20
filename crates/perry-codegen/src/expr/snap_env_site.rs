//! SNAP census: the consumer kind of a `process.env` read site
//! (`secret-tests/nightly/CW/SNAP_DESIGN_2026-10-30.md` §2.10.5), emitted in
//! instrumented compiles only (`PERRY_SNAP_CENSUS` set while compiling, the
//! switch `js_snap_entry_body` uses). An ordinary compile emits nothing.
//!
//! A read is a *store* site when its value is directly the stored value of a
//! module-global binding (`let`/`const` lowered to a module global, an
//! assignment to one, `GlobalSet`), a property or index set, or an element of
//! an array or object literal. Every other read is an *observe* site. Codegen
//! calls `js_snap_env_site(kind)` right before the read's runtime call; the
//! runtime keeps the kind for that frame's next `js_getenv`. No table is
//! emitted: the kind is the call's argument.
//!
//! The read classifies itself against its own parent: [`Lowering`] holds the
//! two innermost expressions `lower_expr` is lowering, installed for the
//! extent of each lowering and restored when it returns, so the parent seen
//! by a read is the expression whose lowering is lowering it, and nothing
//! outlives that lowering. `process.argv` reads use the same frame
//! (`snap_argv_site`).

use perry_hir::Expr;

use crate::types::I32;

use super::FnCtx;

/// `js_snap_env_site` kinds (the runtime reads the same numbers).
const OBSERVE: i32 = 0;
const STORE: i32 = 1;

/// One node of the census lowering frame.
#[derive(Clone, Copy, Default)]
enum Node {
    #[default]
    None,
    /// An expression whose lowering is in flight.
    Expr(*const Expr),
    /// A module-global `let`/`const` binding whose initializer is this
    /// expression is being lowered.
    StoredInit(*const Expr),
}

/// The census lowering frame: `[parent, current]` of the innermost
/// `lower_expr`. A field of the function's [`FnCtx`], saved and restored by
/// every lowering ([`enter`]), never a list.
#[derive(Clone, Copy, Default)]
pub(crate) struct Lowering {
    parent: Node,
    current: Node,
}

impl Lowering {
    /// The parent of `expr` when `expr` is the expression being lowered.
    fn parent_of(&self, expr: &Expr) -> Option<&Expr> {
        let Node::Expr(current) = self.current else {
            return None;
        };
        if !std::ptr::eq(current, expr) {
            return None;
        }
        match self.parent {
            // SAFETY: `parent` was installed as `current` by the enclosing
            // `lower_expr(ctx, parent)` call, whose `&Expr` borrow is live
            // for the whole lowering of its operands; that call restores the
            // frame before it returns, so the pointer is never read after
            // the borrow ends.
            Node::Expr(p) => Some(unsafe { &*p }),
            _ => None,
        }
    }

    /// `expr`, being lowered, is the initializer of a module-global binding.
    fn is_stored_init(&self, expr: &Expr) -> bool {
        matches!(self.current, Node::Expr(c) if std::ptr::eq(c, expr))
            && matches!(self.parent, Node::StoredInit(i) if std::ptr::eq(i, expr))
    }
}

pub(crate) fn census_compile() -> bool {
    std::env::var_os("PERRY_SNAP_CENSUS").is_some_and(|v| !v.is_empty())
}

/// `lower_expr(ctx, expr)` starts: `expr` becomes the current node. Returns
/// the frame to restore when it ends.
#[inline]
pub(crate) fn enter(ctx: &mut FnCtx<'_>, expr: &Expr) -> Lowering {
    let saved = ctx.snap_lowering;
    ctx.snap_lowering = Lowering {
        parent: saved.current,
        current: Node::Expr(expr),
    };
    saved
}

/// `f` lowers one operand of `parent` on a path that lowers `parent`'s
/// operands directly instead of through `lower_expr(ctx, parent)` (scalar
/// replacement of an array or object literal binding): `parent` is the
/// current node for the extent of `f`, as `lower_expr(ctx, parent)` would
/// install it, and the frame is restored when `f` returns.
pub(crate) fn operand_of<'a, R>(
    ctx: &mut FnCtx<'a>,
    parent: &Expr,
    f: impl FnOnce(&mut FnCtx<'a>) -> R,
) -> R {
    let saved = enter(ctx, parent);
    let result = f(ctx);
    ctx.snap_lowering = saved;
    result
}

/// A `let`/`const` binding `id` with initializer `init` is about to be
/// lowered. Returns the frame to restore once it is.
pub(crate) fn enter_let(ctx: &mut FnCtx<'_>, id: u32, init: Option<&Expr>) -> Lowering {
    let saved = ctx.snap_lowering;
    if let Some(init) = init.filter(|_| ctx.module_globals.contains_key(&id)) {
        ctx.snap_lowering = Lowering {
            parent: Node::None,
            current: Node::StoredInit(init),
        };
    }
    saved
}

/// `parent` stores the value of its direct child `child`.
fn stores(ctx: &FnCtx<'_>, parent: &Expr, child: &Expr) -> bool {
    let is = |e: &Expr| std::ptr::eq(e, child);
    match parent {
        Expr::GlobalSet(_, v) => is(&**v),
        Expr::LocalSet(id, v) => ctx.module_globals.contains_key(id) && is(&**v),
        Expr::PropertySet { value, .. } | Expr::IndexSet { value, .. } => is(&**value),
        Expr::Object(fields) => fields.iter().any(|(_, v)| is(v)),
        // A plain object literal lowers to `new __AnonShape_*(values...)`:
        // the literal shape has no constructor, its values are its fields.
        Expr::New {
            class_name, args, ..
        } => {
            ctx.classes
                .get(class_name)
                .is_some_and(|c| c.is_literal_shape())
                && args.iter().any(|a| is(a))
        }
        Expr::Array(elems, _) => elems.iter().any(|e| is(e)),
        _ => false,
    }
}

/// Emit the site hook for the env read `expr`, right before its runtime call.
pub(crate) fn emit(ctx: &mut FnCtx<'_>, expr: &Expr) {
    if !census_compile() {
        return;
    }
    let frame = ctx.snap_lowering;
    let store = frame.is_stored_init(expr)
        || frame
            .parent_of(expr)
            .is_some_and(|parent| stores(ctx, parent, expr));
    let kind = if store { STORE } else { OBSERVE };
    ctx.block()
        .call_void("js_snap_env_site", &[(I32, &kind.to_string())]);
}

/// The parent of the `process.argv` read `expr` being lowered (`snap_argv_site`).
pub(crate) fn parent_of<'f>(ctx: &'f FnCtx<'_>, expr: &Expr) -> Option<&'f Expr> {
    ctx.snap_lowering.parent_of(expr)
}

#[cfg(test)]
#[path = "snap_env_site_tests.rs"]
mod tests;
