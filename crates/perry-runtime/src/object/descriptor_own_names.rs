//! Own-property-name normalization that would push `descriptors.rs` over the
//! repository's 2,000-line cap.

use super::*;

/// Put an Error subclass's lazy `stack` slot ahead of `message`, matching V8.
pub(crate) unsafe fn normalize_error_subclass_own_names(
    obj: *const ObjectHeader,
    names: &mut Vec<String>,
) {
    if (*obj).class_id == 0 || !extends_builtin_error((*obj).class_id) {
        return;
    }
    let mut insert_at = names
        .iter()
        .position(|name| canonical_array_index(name).is_none())
        .unwrap_or(names.len());
    for special in ["stack", "message"] {
        if let Some(index) = names.iter().position(|name| name == special) {
            let name = names.remove(index);
            if index < insert_at {
                insert_at -= 1;
            }
            names.insert(insert_at, name);
            insert_at += 1;
        }
    }
}
