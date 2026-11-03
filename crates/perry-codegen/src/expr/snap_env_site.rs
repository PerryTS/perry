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

use std::cell::RefCell;

use perry_hir::Expr;

use crate::types::I32;

use super::FnCtx;

/// `js_snap_env_site` kinds (the runtime reads the same numbers).
const OBSERVE: i32 = 0;
const STORE: i32 = 1;

thread_local! {
    /// Env-read expressions (by address) whose parent stores their value,
    /// noted when the parent is lowered and taken when the read is.
    static STORE_SITES: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}

pub(crate) fn census_compile() -> bool {
    std::env::var_os("PERRY_SNAP_CENSUS").is_some_and(|v| !v.is_empty())
}

fn is_env_read(e: &Expr) -> bool {
    matches!(e, Expr::EnvGet(_) | Expr::EnvGetDynamic(_))
}

fn note<'e>(children: impl Iterator<Item = &'e Expr> + Clone) {
    if children.clone().any(is_env_read) && census_compile() {
        STORE_SITES.with(|s| {
            let mut s = s.borrow_mut();
            for c in children.filter(|c| is_env_read(c)) {
                s.push(c as *const Expr as usize);
            }
        });
    }
}

/// `expr` is about to be lowered: when it stores the value of a direct
/// env-read child, that child is a store site.
pub(crate) fn note_store_parent(ctx: &FnCtx<'_>, expr: &Expr) {
    match expr {
        Expr::GlobalSet(_, v) => note(std::iter::once(&**v)),
        Expr::LocalSet(id, v) if ctx.module_globals.contains_key(id) => note(std::iter::once(&**v)),
        Expr::PropertySet { value, .. } | Expr::IndexSet { value, .. } => {
            note(std::iter::once(&**value))
        }
        Expr::Object(fields) => note(fields.iter().map(|(_, v)| v)),
        // A plain object literal lowers to `new __AnonShape_*(values...)`:
        // the literal shape has no constructor, its values are its fields.
        Expr::New {
            class_name, args, ..
        } if ctx
            .classes
            .get(class_name)
            .is_some_and(|c| c.is_literal_shape()) =>
        {
            note(args.iter())
        }
        Expr::Array(elems, _) => note(elems.iter()),
        _ => {}
    }
}

/// A `let`/`const` binding `id` with initializer `init` is being lowered.
pub(crate) fn note_let(ctx: &FnCtx<'_>, id: u32, init: Option<&Expr>) {
    if let Some(init) = init {
        if ctx.module_globals.contains_key(&id) {
            note(std::iter::once(init));
        }
    }
}

/// Emit the site hook for the env read `expr`, right before its runtime call.
pub(crate) fn emit(ctx: &mut FnCtx<'_>, expr: &Expr) {
    if !census_compile() {
        return;
    }
    let addr = expr as *const Expr as usize;
    let store = STORE_SITES.with(|s| {
        let mut s = s.borrow_mut();
        match s.iter().position(|&a| a == addr) {
            Some(i) => {
                s.swap_remove(i);
                true
            }
            None => false,
        }
    });
    let kind = if store { STORE } else { OBSERVE };
    ctx.block()
        .call_void("js_snap_env_site", &[(I32, &kind.to_string())]);
}
