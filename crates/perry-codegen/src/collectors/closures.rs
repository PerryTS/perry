use std::collections::HashSet;
pub fn collect_closures_in_stmts(
    stmts: &[perry_hir::Stmt],
    seen: &mut HashSet<u32>,
    out: &mut Vec<(u32, perry_hir::Expr)>,
) {
    let mut refs = Vec::new();
    perry_hir::emitted_closures::collect_closure_refs_in_stmts(stmts, seen, &mut refs);
    out.extend(refs.into_iter().map(|(id, e)| (id, e.clone())));
}
pub fn collect_closures_in_expr(
    e: &perry_hir::Expr,
    seen: &mut HashSet<u32>,
    out: &mut Vec<(u32, perry_hir::Expr)>,
) {
    let mut refs = Vec::new();
    perry_hir::emitted_closures::collect_closure_refs_in_expr(e, seen, &mut refs);
    out.extend(refs.into_iter().map(|(id, e)| (id, e.clone())));
}
