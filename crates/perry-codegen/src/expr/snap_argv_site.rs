//! SNAP25 census: the grain of a `process.argv` read, emitted in
//! instrumented compiles only (`PERRY_SNAP_CENSUS` set while compiling). An
//! ordinary compile emits nothing.
//!
//! `process.argv.length` is a length read (kind 1), `process.argv[<constant>]`
//! an element read (kind 2, the index is the call's second argument;
//! `process.execPath` is a metadata property, not an argv read), and every other use lets the
//! whole array escape (kind 0). Codegen calls `js_snap_argv_site(kind, index)`
//! right before `js_process_argv_value`; no table is emitted. The read takes
//! its grain from its own parent, the expression whose lowering is lowering
//! it (the census lowering frame, `snap_env_site::Lowering`).

use perry_hir::Expr;

use crate::types::{I32, I64};

use super::FnCtx;

/// The grain `parent` reads of its direct `process.argv` child `argv`.
fn grain(parent: &Expr, argv: &Expr) -> Option<(i32, i64)> {
    match parent {
        Expr::PropertyGet {
            object, property, ..
        } if std::ptr::eq(&**object, argv) && property == "length" => Some((1, 0)),
        Expr::IndexGet { object, index, .. } if std::ptr::eq(&**object, argv) => match **index {
            Expr::Number(n) if n >= 0.0 && n.fract() == 0.0 && n < 1e9 => Some((2, n as i64)),
            // Literal indices that fit an integer lower as Expr::Integer.
            Expr::Integer(n) if n >= 0 => Some((2, n)),
            _ => None,
        },
        _ => None,
    }
}

/// Emit the site hook for the `process.argv` read `expr`, right before its runtime call.
pub(crate) fn emit(ctx: &mut FnCtx<'_>, expr: &Expr) {
    if !super::snap_env_site::census_compile() {
        return;
    }
    let (kind, index) = super::snap_env_site::parent_of(ctx, expr)
        .and_then(|parent| grain(parent, expr))
        .unwrap_or((0, 0));
    ctx.block().call_void(
        "js_snap_argv_site",
        &[(I32, &kind.to_string()), (I64, &index.to_string())],
    );
}
