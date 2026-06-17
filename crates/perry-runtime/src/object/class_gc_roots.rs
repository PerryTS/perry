//! GC mutable-root scanner for the class static-inheritance side-tables
//! (issue #1790, epic #1785 / design #1772).
//!
//! Split out of `object/class_registry.rs` to keep that file under the 2,000-
//! line CI gate. The two tables live in `class_registry`; this module only
//! holds the GC scanner (and its test seeds) that root + relocate their
//! pointer values.

use super::class_registry::{
    CLASS_DECL_PROTOTYPE_OBJECTS, CLASS_PARENT_CLOSURES, CLASS_PROTOTYPE_OBJECTS,
};

/// GC mutable-root scanner for the class static-inheritance side-tables
/// (issue #1790, epic #1785 / design #1772).
///
/// `CLASS_PROTOTYPE_OBJECTS` (`class Sub extends make(...) {}` parent class
/// objects, `Object.create(proto)` prototypes, and `Function.prototype = obj`
/// objects) and `CLASS_PARENT_CLOSURES` (`class Svc extends Context.Tag(..)()`
/// closure parents, #36/#321) both store the heap parent as a *raw* `usize`
/// user pointer — invisible to the GC before this scanner. A parent reachable
/// *only* through one of these tables would be:
///   - SWEPT (not a root → freed) under any collection that otherwise can't
///     reach it, after which the static-inheritance walk
///     (`resolve_proto_chain_field` / `resolve_proto_chain_symbol` /
///     `class_parent_closure`) dereferences freed memory; or
///   - DANGLING after a #1095 copying-nursery / C4b evacuation moved it — the
///     stored address would point at the stale/forwarded location.
///
/// This is the same rooting-hazard class the integer-hack design (#1772) was
/// rejected for, and a sibling of the `IMPLICIT_THIS` fix in #1813
/// (`scan_implicit_this_roots_mut`). The keys are scalar `class_id`s (no GC
/// concern); only the pointer *values* are visited. `CLASS_STATIC_METHODS`
/// stores code `func_ptr`s, not heap pointers, so it is deliberately not
/// scanned here.
///
/// `visit_usize_slot` does the right thing in every phase: in mark / copying
/// modes it treats the value as a live root (so the parent survives), and in
/// rewrite / verify modes it follows the forwarding pointer and updates the
/// stored address in place (so the static-inheritance walk resolves correctly
/// AFTER evacuation). A non-pointer / freed slot that isn't in the valid set
/// flows through as a no-op. The maps are decided by the GC owner (#1096) to
/// stay as side-tables + rooted here rather than moved in-object, since the
/// inheritance walk reads them by `class_id`, not by an in-object slot.
pub fn scan_class_inheritance_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    if let Ok(mut guard) = CLASS_PROTOTYPE_OBJECTS.write() {
        if let Some(map) = guard.as_mut() {
            for ptr in map.values_mut() {
                visitor.visit_usize_slot(ptr);
            }
        }
    }
    if let Ok(mut guard) = CLASS_DECL_PROTOTYPE_OBJECTS.write() {
        if let Some(map) = guard.as_mut() {
            for ptr in map.values_mut() {
                visitor.visit_usize_slot(ptr);
            }
        }
    }
    if let Ok(mut guard) = CLASS_PARENT_CLOSURES.write() {
        if let Some(map) = guard.as_mut() {
            for ptr in map.values_mut() {
                visitor.visit_usize_slot(ptr);
            }
        }
    }
}

#[cfg(test)]
pub(crate) fn test_seed_class_inheritance_roots(proto_cid: u32, proto_ptr: usize) {
    // GC_STORE_AUDIT(ROOT): test seed mirrors CLASS_PROTOTYPE_OBJECTS values scanned by scan_class_inheritance_roots_mut.
    let mut guard = CLASS_PROTOTYPE_OBJECTS.write().unwrap();
    guard
        .get_or_insert_with(std::collections::HashMap::new)
        .insert(proto_cid, proto_ptr);
}

#[cfg(test)]
pub(crate) fn test_seed_decl_class_prototype_root(class_id: u32, proto_ptr: usize) {
    let mut guard = CLASS_DECL_PROTOTYPE_OBJECTS.write().unwrap();
    guard
        .get_or_insert_with(std::collections::HashMap::new)
        .insert(class_id, proto_ptr);
}

#[cfg(test)]
pub(crate) fn test_seed_class_parent_closure_root(closure_cid: u32, closure_ptr: usize) {
    // GC_STORE_AUDIT(ROOT): test seed mirrors CLASS_PARENT_CLOSURES values scanned by scan_class_inheritance_roots_mut.
    let mut guard = CLASS_PARENT_CLOSURES.write().unwrap();
    guard
        .get_or_insert_with(std::collections::HashMap::new)
        .insert(closure_cid, closure_ptr);
}

#[cfg(test)]
pub(crate) fn test_class_prototype_object_root(proto_cid: u32) -> usize {
    CLASS_PROTOTYPE_OBJECTS
        .read()
        .unwrap()
        .as_ref()
        .and_then(|m| m.get(&proto_cid).copied())
        .unwrap_or(0)
}

#[cfg(test)]
pub(crate) fn test_decl_class_prototype_root(class_id: u32) -> usize {
    CLASS_DECL_PROTOTYPE_OBJECTS
        .read()
        .unwrap()
        .as_ref()
        .and_then(|m| m.get(&class_id).copied())
        .unwrap_or(0)
}

#[cfg(test)]
pub(crate) fn test_class_parent_closure_root(closure_cid: u32) -> usize {
    CLASS_PARENT_CLOSURES
        .read()
        .unwrap()
        .as_ref()
        .and_then(|m| m.get(&closure_cid).copied())
        .unwrap_or(0)
}

#[cfg(test)]
pub(crate) fn test_clear_class_inheritance_roots(proto_cid: u32, closure_cid: u32) {
    if let Some(m) = CLASS_PROTOTYPE_OBJECTS.write().unwrap().as_mut() {
        m.remove(&proto_cid);
    }
    if let Some(m) = CLASS_DECL_PROTOTYPE_OBJECTS.write().unwrap().as_mut() {
        m.remove(&proto_cid);
    }
    if let Some(m) = CLASS_PARENT_CLOSURES.write().unwrap().as_mut() {
        m.remove(&closure_cid);
    }
}
