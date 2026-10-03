//! Prototype-method lookups by (class id, name), own and through the parent chain.

use super::*;

/// Lookup helper: returns the registered prototype-method value for
/// `(class_id, name)`, or None if no assignment matched. Walks the
/// parent-class chain so methods registered on a base class are found
/// via subclass instances.
pub(crate) fn lookup_own_prototype_method(class_id: u32, name: &str) -> Option<f64> {
    CLASS_PROTOTYPE_METHODS.with(|table| {
        let guard = table.read().ok()?;
        let bits = guard.as_ref()?.get(&class_id)?.get(name)?;
        Some(f64::from_bits(*bits))
    })
}

pub(crate) fn lookup_prototype_method(class_id: u32, name: &str) -> Option<f64> {
    CLASS_PROTOTYPE_METHODS.with(|table| {
        let guard = table.read().ok()?;
        let map = guard.as_ref()?;
        let mut cid = class_id;
        let mut depth = 0usize;
        while depth < 32 {
            if !class_proto_key_deleted(cid, name) {
                if let Some(per_class) = map.get(&cid) {
                    if let Some(&bits) = per_class.get(name) {
                        return Some(f64::from_bits(bits));
                    }
                }
            }
            // A relinked `C.prototype` no longer inherits from the parent
            // class's prototype, so the parent's assignments are off its chain.
            let next = match crate::object::class_generic_origin(cid) {
                Some(origin) => Some(origin),
                None if super::super::class_decl_prototype_relinked(cid) => None,
                None => get_parent_class_id(cid),
            };
            match next {
                Some(p) if p != 0 && p != cid => {
                    cid = p;
                    depth += 1;
                }
                _ => break,
            }
        }
        None
    })
}
