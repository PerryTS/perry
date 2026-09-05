//! The minor-scoped half of the string-keyed descriptor root scan.
//!
//! `scan_descriptor_roots_mut` walks every owner in `attr_keys_by_owner` and
//! `accessor_keys_by_owner`; on a minor that is ~23k owners to discover that
//! none of them points at the nursery. The young-entry log
//! (`gc/young_log.rs`) narrows a minor-scoped pass to the owners a minor can
//! act on, and this module holds that pass plus the re-derivation of the
//! relevant set that rule 2 checks it against under `debug_assertions`.

use super::*;

/// Every owner whose entry can matter to a minor, re-derived from the
/// authoritative tables: a non-old owner, or an accessor whose getter or
/// setter is non-old.
pub(super) fn relevant_descriptor_owners(st: &crate::state::RuntimeState) -> Vec<usize> {
    use crate::gc::young_log::{addr_is_minor_relevant, bits_are_minor_relevant};
    let mut relevant = Vec::new();
    for &owner in st.descriptors.attr_keys_by_owner.borrow().keys() {
        if addr_is_minor_relevant(owner) {
            relevant.push(owner);
        }
    }
    for &owner in st.descriptors.accessor_keys_by_owner.borrow().keys() {
        if addr_is_minor_relevant(owner) {
            relevant.push(owner);
        }
    }
    for ((owner, _), acc) in st.descriptors.accessor_descriptors.borrow().iter() {
        if bits_are_minor_relevant(acc.get) || bits_are_minor_relevant(acc.set) {
            relevant.push(*owner);
        }
    }
    relevant.sort_unstable();
    relevant.dedup();
    relevant
}

/// The minor-scoped walk (#9754): only the young-logged owners, each visited
/// exactly as the full walk visits it — accessor get/set rooted in every
/// phase, owner re-keyed across both tables and both indexes in the rewrite
/// phase — and re-logged iff still relevant afterwards.
pub(super) fn scan_descriptor_roots_young(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    st: &crate::state::RuntimeState,
) {
    let table_len = st.descriptors.attr_keys_by_owner.borrow().len() as u64
        + st.descriptors.accessor_keys_by_owner.borrow().len() as u64;
    #[cfg(debug_assertions)]
    {
        let relevant = relevant_descriptor_owners(st);
        st.descriptors
            .young_owners
            .borrow()
            .debug_assert_logged(DESCRIPTOR_YOUNG_LOG_NAME, &relevant);
    }
    let mut logged = 0u64;
    let mut visited = 0u64;
    let mut kept = Vec::new();
    loop {
        let batch = st.descriptors.young_owners.borrow_mut().take_sorted();
        if batch.is_empty() {
            break;
        }
        logged += batch.len() as u64;
        for owner in batch {
            visited += 1;
            let (new_owner, relevant) = scan_descriptor_owner(visitor, st, owner);
            if relevant {
                kept.push(new_owner);
            }
        }
    }
    let kept_len = kept.len() as u64;
    st.descriptors.young_owners.borrow_mut().extend(kept);
    crate::gc::young_log::note_walk(
        DESCRIPTOR_YOUNG_LOG_NAME,
        crate::gc::young_log::YoungLogWalk {
            partial: true,
            logged,
            visited,
            kept: kept_len,
            table_len,
        },
    );
}
