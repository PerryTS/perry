//! Stack-map side-table census rows (#9637 `PERRY_GC_CENSUS`), split from
//! `stack_maps.rs` for the 2000-line file cap.

use super::*;

/// `PERRY_GC_CENSUS`: bytes owned by the published stack-map index (the
/// `sections` are `&'static` image bytes and are reported separately as
/// file-backed).
pub(crate) fn stack_map_index_census() -> Vec<crate::gc::census::SideTableRow> {
    use crate::gc::census::vec_bytes;
    let mut rows = Vec::new();
    let Some(lock) = STACK_MAPS.published.get() else {
        return rows;
    };
    let Ok(g) = lock.read() else {
        return rows;
    };
    let ix = &g.index;
    rows.push((
        "stackmap.sections(file-backed)",
        ix.sections.len(),
        ix.sections.iter().map(|s| s.len()).sum(),
    ));
    rows.push((
        "stackmap.functions",
        ix.functions.len(),
        vec_bytes(&ix.functions),
    ));
    if let Some(e) = ix.eager.as_ref() {
        rows.push((
            "stackmap.eager.records",
            e.records.len(),
            vec_bytes(&e.records),
        ));
        rows.push(("stackmap.eager.roots", e.roots.len(), vec_bytes(&e.roots)));
        rows.push((
            "stackmap.eager.derived",
            e.derived.len(),
            vec_bytes(&e.derived),
        ));
        rows.push((
            "stackmap.eager.function_starts",
            e.function_starts.len(),
            vec_bytes(&e.function_starts),
        ));
    }
    rows
}
