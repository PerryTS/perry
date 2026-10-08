//! A declared class's prototype object is born in its final shape.
//!
//! `C.prototype` of a declared class is built once per class per agent, on
//! first demand (`state::class_decl_prototype_value`). Its own keys are a fact
//! of the class: `constructor`, then each ClassBody method in definition
//! order, every one `{ writable, !enumerable, configurable }`, each method
//! slot a function object of the method's own body (its closure-convention
//! entry, a ConstFn lane), linked to the parent's prototype. So the shape it
//! ends in is known before the object exists, exactly as a per-evaluation
//! class's prototype template is (`field_get_set/class_object_template.rs`),
//! except that a declared class needs no first build to record it: the class
//! registry already holds every fact.
//!
//! The birth is therefore one allocation, N slot stores and ONE stamp of the
//! final property shape, which names the link. Publishing the class link then
//! gives the holder the existing private prototype lineage, like any marked
//! chain hop. There is no birth-to-final
//! key-add chain, no attribute claim per key, no relink restamp and no
//! ConstFn relearn, and none of the by-name stores the general path makes
//! (each of which asks whether the object is a class prototype).
//!
//! Accessor pairs carry their key attributes in the final shape too. A class
//! with a method lacking an entry, a method named `constructor`, or a parent without stable
//! identity, takes the general path, which builds the same object key by key.

use super::*;

/// The attribute entry of a ClassBody method and of `constructor`:
/// `{ writable: true, enumerable: false, configurable: true }`.
const METHOD_ENTRY: u8 = crate::object::key_attrs::ENTRY_NON_ENUMERABLE;

/// Build class `class_id`'s prototype object in its final shape, linked to
/// `parent_bits` (the parent prototype, NaN-boxed, or `TAG_NULL`), and make it
/// the class's prototype link. `None` (nothing allocated, nothing linked) when
/// the class's prototype is not one the registry fully describes.
///
/// `class_id` is the prototype's identity id; the caller has minted its class
/// function object and found no prototype linked.
pub(super) fn decl_prototype_born_final(class_id: u32, parent_bits: u64) -> Option<f64> {
    let requested = CLASS_VTABLE_REGISTRY
        .read()
        .ok()
        .and_then(|r| r.as_ref()?.get(&class_id).map(|c| c.prototype_birth_shape))
        .filter(|&id| id != 0);
    let members = class_prototype_member_names(class_id);
    if members.iter().any(|(name, accessor)| {
        name == "constructor" || (!*accessor && class_method_entry(class_id, name).is_none())
    }) || !class_own_symbol_member_keys(class_id, false).is_empty()
    {
        return None;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let parent = scope.root_heap_word_u64(parent_bits);
    // The link's identity is the parent's stable serial: mark it as a
    // prototype first (what every link does to its target), before this
    // object exists. A parent with no serial gets a fresh identity per link,
    // which the general path's funnel mints.
    let serial = if parent_bits == crate::value::TAG_NULL {
        Some(crate::object::proto_validity::NULL_PROTOTYPE_SERIAL)
    } else {
        let value = crate::value::JSValue::from_bits(parent_bits);
        if !value.is_pointer() {
            return None;
        }
        // SAFETY: a live prototype value, rooted in `parent`.
        unsafe {
            crate::object::proto_validity::mark_object_as_prototype(
                value.as_pointer::<ObjectHeader>() as usize,
            )
        }
    };
    serial?;

    // The values, in key order: the constructor (the class's pinned function
    // object), then one entry-backed function object per method (cached and
    // rooted by the class registry; rooted here too across the allocations
    // below).
    let ctor = class_constructor_ref_value(class_id);
    let mut values = Vec::with_capacity(members.len() + 1);
    values.push(scope.root_nanbox_f64(ctor));
    let mut entries = vec![METHOD_ENTRY];
    for (name, accessor) in &members {
        if *accessor {
            use crate::object::accessor_pair::{pair_new, Accessor};
            use crate::object::key_attrs as ka;
            let (raw_get, raw_set) = class_own_accessor_ptrs(class_id, name)?;
            let get = scope.root_nanbox_f64(class_accessor_function_value(
                raw_get, false, false, name, None,
            ));
            let set = scope.root_nanbox_f64(class_accessor_function_value(
                raw_set,
                true,
                false,
                name,
                class_own_setter_length(class_id, name, false),
            ));
            let pair = unsafe {
                pair_new(Accessor {
                    get: get.get_nanbox_f64().to_bits(),
                    set: set.get_nanbox_f64().to_bits(),
                    raw_get,
                    raw_set,
                    static_get: 0,
                    static_set: 0,
                })
            };
            values.push(scope.root_nanbox_f64(crate::value::js_nanbox_pointer(pair as i64)));
            entries.push(
                METHOD_ENTRY
                    | ka::ENTRY_ACCESSOR
                    | if raw_get != 0 { ka::ENTRY_HAS_GET } else { 0 }
                    | if raw_set != 0 { ka::ENTRY_HAS_SET } else { 0 },
            );
            continue;
        }
        let f = class_prototype_method_value_for_name(class_id, name);
        if unsafe { crate::object::field_rep_store::constfn_store_info(f.to_bits()) }.is_none() {
            return None;
        }
        values.push(scope.root_nanbox_f64(f));
        entries.push(METHOD_ENTRY);
    }

    // The canonical key list with its attribute entries: one edge per key.
    let proof = crate::object::canonical_keys::SharedLayout::shape_cache_entry();
    let list = scope.root_raw_mut_ptr::<crate::ArrayHeader>(std::ptr::null_mut());
    let mut count = 0u32;
    for (name, entry) in std::iter::once("constructor")
        .chain(members.iter().map(|(n, _)| n.as_str()))
        .zip(entries)
    {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        // SAFETY: the parent list is rooted in `list`; `extend_key_with_entry`
        // roots its operands across its allocation.
        let next = list.with_mut_ptr(|arr: *mut crate::ArrayHeader| unsafe {
            let parent = crate::object::canonical_keys::CanonicalKeys::from_rooted(arr, count);
            crate::object::canonical_keys::extend_key_with_entry(&proof, parent, key, entry)
        });
        list.set_raw_mut_ptr(next.as_ptr());
        count = next.len();
    }
    if count as usize != values.len() {
        // A duplicate name cannot come from the registry; refuse rather
        // than stamp a shape whose keys miss a value.
        return None;
    }

    // The object and its values, then the one stamp. Its [[Prototype]] is a
    // fact of that shape (the identity names the parent); a fresh object has
    // no meta record to mirror it in. It is fresh and unobserved, so the link
    // is not prototype surgery and invalidates nothing.
    let proto = scope.root_raw_mut_ptr(crate::object::js_object_alloc(class_id, count));
    if proto.with_mut_ptr::<ObjectHeader, _>(|p| p.is_null()) {
        return None;
    }
    if requested.is_some() {
        proto.with_mut_ptr::<ObjectHeader, _>(|p| unsafe {
            crate::object::proto_validity::mark_object_as_prototype(p as usize)
        });
    }
    let parent_bits = parent.get_heap_word_u64();
    let ok = proto.with_mut_ptr(|p: *mut ObjectHeader| unsafe {
        for (slot, value) in values.iter().enumerate() {
            crate::object::slot_store::store_object_field_slot(
                p,
                slot,
                value.get_nanbox_f64().to_bits(),
            );
        }
        let proto_id = if requested.is_some() {
            match get_parent_class_id(class_id) {
                Some(parent) if parent != 0 => {
                    crate::object::shapes::PROTO_ID_CLASS | u64::from(parent)
                }
                _ => crate::object::shapes::PROTO_ID_DEFAULT,
            }
        } else {
            crate::object::shapes::object_proto_id_for(p, parent_bits)
        };
        let stamped = list.with_mut_ptr::<crate::ArrayHeader, _>(|keys| {
            crate::object::shapes::stamp_linked_final_shape_requested(
                p,
                keys,
                count,
                proto_id,
                parent_bits,
                // Slot 0 is `constructor` (a class function object, no lane);
                // every method slot names its body.
                |slot| slot != 0 && !members[slot as usize - 1].1,
                requested,
            )
        });
        if stamped {
            crate::object::descriptor_state::note_attrs_born_with_keys(p as usize);
        }
        stamped
    });
    // The stamp refuses only facts this builder never produces; the object
    // stays unlinked and unreachable, and the general path builds another.
    if !ok {
        return None;
    }
    crate::gc::runtime_shade_external_edge(parent_bits);
    proto.with_mut_ptr::<ObjectHeader, _>(|p| {
        super::state::class_decl_prototype_object_root_store(class_id, p)
    });
    #[cfg(test)]
    BORN_FINAL_BUILDS.with(|n| n.set(n.get() + 1));
    Some(proto.with_mut_ptr::<ObjectHeader, _>(|p| crate::value::js_nanbox_pointer(p as i64)))
}

#[cfg(test)]
thread_local! {
    /// Prototypes this thread built born-final (tests prove which path ran).
    pub(super) static BORN_FINAL_BUILDS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}
