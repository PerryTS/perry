//! SNAP25 census: the grain of a `process.argv` read, emitted in
//! instrumented compiles only (`PERRY_SNAP_CENSUS` set while compiling). An
//! ordinary compile emits nothing.
//!
//! `process.argv.length` is a length read (kind 1), `process.argv[<constant>]`
//! an element read (kind 2, the index is the call's second argument;
//! `process.execPath` is a metadata property, not an argv read), and every other use lets the
//! whole array escape (kind 0). Codegen calls `js_snap_argv_site(kind, index)`
//! right before `js_process_argv_value`; no table is emitted.

use std::cell::RefCell;

use perry_hir::Expr;

use crate::types::{I32, I64};

use super::FnCtx;

thread_local! {
    /// `process.argv` expressions (by address) whose parent reads one grain.
    static SITES: RefCell<Vec<(usize, i32, i64)>> = const { RefCell::new(Vec::new()) };
}

/// `expr` is about to be lowered: note the grain of a direct `process.argv` child.
pub(crate) fn note_parent(expr: &Expr) {
    if !super::snap_env_site::census_compile() {
        return;
    }
    let site = match expr {
        Expr::PropertyGet {
            object, property, ..
        } if matches!(**object, Expr::ProcessArgv) && property == "length" => {
            Some((&**object, 1, 0))
        }
        Expr::IndexGet { object, index, .. } if matches!(**object, Expr::ProcessArgv) => {
            match **index {
                Expr::Number(n) if n >= 0.0 && n.fract() == 0.0 && n < 1e9 => {
                    Some((&**object, 2, n as i64))
                }
                // Literal indices that fit an integer lower as Expr::Integer.
                Expr::Integer(n) if n >= 0 => Some((&**object, 2, n)),
                _ => None,
            }
        }
        _ => None,
    };
    if let Some((child, kind, index)) = site {
        SITES.with(|s| s.borrow_mut().push((child as *const Expr as usize, kind, index)));
    }
}

/// Emit the site hook for the `process.argv` read `expr`, right before its runtime call.
pub(crate) fn emit(ctx: &mut FnCtx<'_>, expr: &Expr) {
    if !super::snap_env_site::census_compile() {
        return;
    }
    let addr = expr as *const Expr as usize;
    let (kind, index) = SITES.with(|s| {
        let mut s = s.borrow_mut();
        match s.iter().position(|&(a, _, _)| a == addr) {
            Some(i) => {
                let (_, k, x) = s.swap_remove(i);
                (k, x)
            }
            None => (0, 0),
        }
    });
    ctx.block().call_void(
        "js_snap_argv_site",
        &[(I32, &kind.to_string()), (I64, &index.to_string())],
    );
}
