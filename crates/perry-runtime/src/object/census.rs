//! `PERRY_GC_CENSUS` reporting for the per-thread object side tables.
//!
//! Split out of `object/mod.rs` to keep that file under the 2000-line size
//! gate (`scripts/check_file_size.sh`). This is the whole of the object
//! module's census surface: it reads `RuntimeState`'s object tables and the
//! two fixed-size caches and renders them as `gc::census::SideTableRow`s. It
//! is a reporting leaf — nothing in the object module calls it, and its only
//! caller is `gc/census.rs`'s aggregator.

use super::*;

/// `PERRY_GC_CENSUS`: per-thread object tables (`RuntimeState`): the fixed
/// caches, the overflow-field vectors and the descriptor tables.
pub(crate) fn object_tables_census() -> Vec<crate::gc::census::SideTableRow> {
    use crate::gc::census::{map_bytes, vec_bytes};
    let st = crate::state::state();
    let mut rows: Vec<crate::gc::census::SideTableRow> = Vec::new();
    {
        let m = st.object_hot.overflow_fields.borrow();
        let inner: usize = m.values().map(vec_bytes).sum();
        rows.push(("object.overflow_fields", m.len(), map_bytes(&m) + inner));
    }
    rows.push((
        "object.transition_cache(fixed)",
        TRANSITION_CACHE_SIZE,
        TRANSITION_CACHE_SIZE * std::mem::size_of::<TransitionEntry>(),
    ));
    rows.push((
        "object.shape_inline_cache(fixed)",
        SHAPE_INLINE_CACHE_SIZE,
        SHAPE_INLINE_CACHE_SIZE * std::mem::size_of::<ShapeCacheEntry>(),
    ));
    {
        let m = st.descriptors.property_descriptors.borrow();
        let inner: usize = m.keys().map(|(_, k)| k.capacity()).sum();
        rows.push((
            "object.property_descriptors",
            m.len(),
            map_bytes(&m) + inner,
        ));
    }
    {
        let m = st.descriptors.accessor_descriptors.borrow();
        let inner: usize = m.keys().map(|(_, k)| k.capacity()).sum();
        rows.push((
            "object.accessor_descriptors",
            m.len(),
            map_bytes(&m) + inner,
        ));
    }
    rows
}
