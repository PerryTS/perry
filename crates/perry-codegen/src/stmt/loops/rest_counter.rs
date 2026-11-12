//! I32 at rest B2: the at-rest locals a counted loop indexes by its counter,
//! for the counter entry check (`counter_entry_check`), which proves every
//! counter value below each local's floor in the fast copy
//! (`FnCtx::rest_counter`, `expr::rest_elems::counter_in_floor`).

use perry_hir::Stmt;

use crate::expr::FnCtx;

/// The at-rest locals a body indexes by exactly `counter` (or `counter |
/// 0`) at an at-rest element site (`expr::rest_elems`), with each local's
/// floor; the body never writes the local, so the Array it holds at entry
/// is the one every access reads.
pub(super) fn collect(ctx: &FnCtx<'_>, body: &[Stmt], counter_id: u32) -> Vec<(u32, u32)> {
    use perry_hir::Expr;
    fn visit(ctx: &FnCtx<'_>, body: &[Stmt], e: &Expr, counter_id: u32, out: &mut Vec<(u32, u32)>) {
        let site = crate::expr::rest_elems::read_floor(ctx, e)
            .or_else(|| crate::expr::rest_elems::write_floor(ctx, e))
            .is_some();
        let access = match e {
            Expr::IndexGet { object, index, .. }
            | Expr::IndexSet { object, index, .. }
            | Expr::PutValueSet {
                target: object,
                key: index,
                ..
            } => Some((object.as_ref(), index.as_ref())),
            _ => None,
        };
        if let (true, Some((Expr::LocalGet(id), index))) = (site, access) {
            if crate::expr::rest_elems::counter_of(index) == Some(counter_id)
                && !super::stmts_mutate_local(body, *id)
                && !out.iter().any(|&(l, _)| l == *id)
            {
                if let Some(floor) = crate::expr::rest_elems::local_floor(ctx, *id) {
                    out.push((*id, floor));
                }
            }
        }
        perry_hir::walker::walk_expr_children(e, &mut |c| visit(ctx, body, c, counter_id, out));
    }
    let mut out = Vec::new();
    if !crate::expr::rest_elems::tiers_enabled() {
        return out;
    }
    let mut exprs = Vec::new();
    super::collect_stmt_exprs(body, &mut exprs);
    for e in exprs {
        visit(ctx, body, e, counter_id, &mut out);
    }
    out
}
