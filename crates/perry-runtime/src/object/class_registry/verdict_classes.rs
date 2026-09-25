//! The class side of a store site's chain verdict: which class ids a verdict
//! walked, and the validity bump an accessor registration on one of them owes.
//! Split out of `parent_static.rs` (file-size cap).

use super::*;

/// Class ids some chain verdict walked (`class_chain_has_instance_accessor`
/// through a store site's key-add memo). The class-side twin of a MARKED
/// prototype: an accessor registered for one of these classes changes what a
/// recorded verdict answers, so it moves `proto_validity`; an accessor on a
/// class no verdict walked — every class evaluated for the first time — moves
/// nothing, because no memo can depend on a class none of its receivers had.
static VERDICT_CLASSES: std::sync::RwLock<Option<std::collections::HashSet<u32>>> =
    std::sync::RwLock::new(None);

/// Mark `class_id` and every class-id ancestor as walked by a verdict. Called
/// BEFORE the verdict's generation is read, so a registration racing the
/// verdict either bumps the word the memo records or lands before it.
pub(crate) fn mark_class_chain_for_verdicts(class_id: u32) {
    let mut cid = class_id;
    let mut depth = 0usize;
    while cid != 0 && depth < 32 {
        let known = VERDICT_CLASSES
            .read()
            .ok()
            .is_some_and(|set| set.as_ref().is_some_and(|set| set.contains(&cid)));
        if !known {
            if let Ok(mut set) = VERDICT_CLASSES.write() {
                set.get_or_insert_with(Default::default).insert(cid);
            }
        }
        match get_parent_class_id(cid) {
            Some(p) if p != 0 && p != cid => {
                cid = p;
                depth += 1;
            }
            _ => break,
        }
    }
}

/// An instance accessor was registered for `class_id`: move the validity word
/// if a verdict walked that class.
pub(crate) fn note_verdict_class_accessor_change(class_id: u32) {
    let walked = VERDICT_CLASSES
        .read()
        .ok()
        .is_some_and(|set| set.as_ref().is_some_and(|set| set.contains(&class_id)));
    if walked {
        crate::object::proto_validity::bump_proto_validity();
    }
}
