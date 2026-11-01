//! A function object's own data put answered from its shape (GPP4).
//!
//! A function's own string keys live in its bag (`closure::props`), an
//! ordinary null-prototype object whose key list IS the closure's keyed
//! Function shape's key list: the closure's ShapeId names the bag's keys,
//! count and live inline bound, the Function kind and the function's
//! [[Prototype]] identity (`closure::shape::refresh_closure_shape`). So a
//! define or store of a plain data key is an ordinary shape-owned key-add on
//! that list, and the closure's next ShapeId is the key-add edge of its own
//! ShapeId:
//!
//! 1. the layouts answer the put (`layout_probe::function_put_plan`): the
//!    bag's own position for the key (hashed, by atom), and for `[[Set]]`
//!    of an absent key the chain from the function's link, which its shape
//!    names (`chain_admits_add` from the link object);
//! 2. the bag takes the key through its published key-add edge
//!    (`leaf_key_add`, inline slot) or the chain-proven transition (spill);
//! 3. the closure moves to the target of its own ShapeId's key-add edge in
//!    the transition cache, validated per use against the bag's keys, count
//!    and live bound; a miss remints (`refresh_closure_shape`) and teaches
//!    the edge.
//!
//! Declined (`None`, nothing written): a FunctionDictionary or class function
//! (accessors, symbols, deletes, a recorded link live on the object there), a
//! frozen, sealed or non-extensible function, a key that is not an interned
//! named key, the intrinsic keys (`name`, `length`, `prototype`, `caller`,
//! `arguments`, `__proto__`), an own key with other attributes, a chain the
//! layouts cannot clear, a bag with an overflow or tombstones. The general
//! put runs then and teaches the edges for the next function of this shape.
//!
//! Runs under `GcSuppressScope`, as every bag writer does (`closure::props`):
//! nothing moves, so raw addresses stay valid across the spill growth.
use super::ObjectHeader;
use crate::closure::ClosureHeader;
use crate::codegen_abi::LayoutWord;
use crate::object::layout_probe::FunctionPut;

/// Is `key` (a live string) an intrinsic key of a function object, which has
/// its own materialization and attribute rules?
#[inline]
unsafe fn intrinsic_function_key(key: *const crate::StringHeader) -> bool {
    let bytes = std::slice::from_raw_parts(
        crate::string::string_data(key),
        (*key).byte_len as usize,
    );
    matches!(
        bytes,
        b"name" | b"length" | b"prototype" | b"caller" | b"arguments" | b"__proto__"
    ) || bytes.first() == Some(&b'#')
}

/// CreateDataProperty (`define`) or `[[Set]]` of `key_value` = `value` on the
/// function object `target`, from its shape (module docs). `Some(value)` when
/// done, `None` with nothing written.
///
/// # Safety
/// `target`, `key_value` and `value` are live values.
#[inline(never)]
pub(crate) unsafe fn function_own_put(
    target: f64,
    key_value: f64,
    value: f64,
    define: bool,
) -> Option<f64> {
    let bits = target.to_bits();
    if bits & !crate::value::POINTER_MASK != crate::value::POINTER_TAG {
        return None;
    }
    let ptr = (bits & crate::value::POINTER_MASK) as usize;
    if !crate::value::addr_class::is_above_handle_band(ptr) {
        return None;
    }
    let header = crate::value::addr_class::try_read_gc_header(ptr)?;
    if header.obj_type != crate::gc::GC_TYPE_CLOSURE
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || header._reserved
            & (crate::gc::OBJ_FLAG_FROZEN | crate::gc::OBJ_FLAG_SEALED | crate::gc::OBJ_FLAG_NO_EXTEND)
            != 0
    {
        return None;
    }
    let c = ptr as *mut ClosureHeader;
    let id = (*c).shape_id;
    if id == crate::closure::shape::function_dictionary_shape()
        || crate::closure::shape::is_class_info((*c).info)
    {
        return None;
    }
    let key = super::store_key::interned_key_for_store(key_value)?;
    if intrinsic_function_key(key) {
        return None;
    }
    // The function's [[Prototype]], from its shape: only `[[Set]]` asks.
    let link = if define {
        crate::value::TAG_NULL
    } else {
        let pid = super::shapes::shape_proto_id(id)?;
        if pid == super::shapes::PROTO_ID_NULL {
            crate::value::TAG_NULL
        } else {
            match super::shapes::identity_prototype_word(pid) {
                0 => return None,
                word => word,
            }
        }
    };
    let _no_move = crate::gc::GcSuppressScope::new();
    let bag = (*c).props;
    if bag.is_null() {
        // The first own key: the bag is born holding the intrinsic slots and
        // this key in one shape (`props::bag_born_with_attrs`).
        if !define && !super::layout_probe::function_link_admits_add(link, key) {
            return None;
        }
        let name = crate::string::header_str_checked(key)?;
        let prev = (*c).shape_id;
        if !crate::closure::props::bag_born_with_attrs(ptr, &[(name, value)], &[]) {
            return None;
        }
        // The born bag lists the intrinsic slots and `key`: the closure's
        // next shape is `prev`'s key-add edge for `key`, as for any add.
        closure_shape_key_added(c, prev, key);
        return Some(value);
    }
    match super::layout_probe::function_put_plan(bag, link, key, define) {
        FunctionPut::Decline => None,
        FunctionPut::Overwrite => {
            let bytes = std::slice::from_raw_parts(
                crate::string::string_data(key),
                (*key).byte_len as usize,
            );
            super::try_existing_own_data_overwrite_by_content(bag, key_value, bytes, value)
                .then_some(value)
        }
        FunctionPut::Add => {
            let prev = (*c).shape_id;
            if !super::leaf_key_add(bag, key, value.to_bits()) {
                if super::shapes::object_shape_record(bag)?.object_kind()
                    == super::shapes::ShapeObjectKind::OrdinaryUnmarked
                {
                    super::alloc_plain::mark_object_plain_ordinary(bag);
                }
                let mut refresh = None;
                super::object_set_field_by_name_transition_chain_proven_value(
                    bag,
                    key,
                    value,
                    &mut refresh,
                )?;
            }
            closure_shape_key_added(c, prev, key);
            Some(value)
        }
    }
}

/// Move the closure `c`, whose bag just took `key` while `c` carried the
/// keyed Function ShapeId `prev`, to its next ShapeId: the target of `prev`'s
/// key-add edge when that target describes the bag as it now is (its keys,
/// count and live inline bound; kind and link identity are `prev`'s, which
/// the edge is keyed on), else the reminted shape, whose edge is then taught.
///
/// # Safety
/// `c` is a live described closure whose bag is non-null.
unsafe fn closure_shape_key_added(
    c: *mut ClosureHeader,
    prev: LayoutWord,
    key: *const crate::StringHeader,
) {
    let bag = (*c).props as *const ObjectHeader;
    let Some(now) = super::shapes::object_shape_record(bag) else {
        crate::closure::shape::refresh_closure_shape(c as usize);
        return;
    };
    if let Some((_, _, target)) = super::transition_cache_lookup(prev, key) {
        if let Some(t) = super::shapes::shape_record_of_receiver(target.0) {
            if t.keys() == now.keys()
                && t.logical_key_count() == now.logical_key_count()
                && t.live_inline_slot_count() == now.live_inline_slot_count()
                && t.object_kind().is_function_layout()
            {
                // GC_STORE_AUDIT(POINTER_FREE): a ShapeId, never a heap reference.
                (*c).shape_id = target;
                return;
            }
        }
    }
    crate::closure::shape::refresh_closure_shape(c as usize);
    let next = (*c).shape_id;
    if next != crate::closure::shape::function_dictionary_shape() && next != prev {
        let count = now.logical_key_count();
        if count != 0 {
            super::transition_cache_insert(
                std::ptr::null(),
                prev,
                key,
                now.keys() as usize,
                count - 1,
                next,
            );
        }
    }
}
