//! #10507: `new F()` and `x instanceof F` for an ordinary compiled function.
//!
//! A function object whose body the compiler emitted (`FN_COMPILED_BODY`)
//! and that is not an arrow, async, generator or class body is an ordinary
//! function: its `[[Construct]]` is OrdinaryCreateFromConstructor plus the
//! body, and its `@@hasInstance` is Function.prototype's (an own one needs a
//! symbol key, which moves the function object to FunctionDictionary). That
//! is a fact of the body, read from the function object's info, so these
//! paths skip every built-in, bound, proxy and native-module probe.
//!
//! The object a construction creates is described by `F.prototype`: its
//! class id and birth ShapeId are the prototype object's birth record
//! (`ObjectMeta::instance_birth`), minted on the first construction by the
//! ordinary allocate-then-link sequence and replayed afterwards — the same
//! class id and the same ShapeId (its `proto_id` is the prototype's serial,
//! its record's `prototype` word the prototype itself) that sequence
//! produces, with no hash lookup and no per-instance record. A reassigned
//! `F.prototype` is read on the next construction; objects already created
//! keep the prototype their shapes name.
use super::*;

use crate::closure::ClosureHeader;
use crate::codegen_abi::{
    FN_ARROW, FN_ASYNC, FN_ASYNC_GENERATOR, FN_BUILTIN, FN_COMPILED_BODY, FN_GENERATOR,
    FN_NON_CONSTRUCTOR,
};

/// Body kinds that make a compiled function something other than an
/// ordinary constructor.
const NOT_ORDINARY: u32 =
    FN_ARROW | FN_ASYNC | FN_GENERATOR | FN_ASYNC_GENERATOR | FN_NON_CONSTRUCTOR | FN_BUILTIN;

/// The function object `value` names when it is an ordinary compiled
/// function (see the module docs), else `None`.
#[inline]
pub(crate) fn ordinary_compiled_function(value: f64) -> Option<usize> {
    let bits = value.to_bits();
    if bits & crate::value::TAG_MASK != crate::value::POINTER_TAG {
        return None;
    }
    let ptr = (bits & crate::value::POINTER_MASK) as usize;
    if !crate::closure::is_closure_ptr(ptr) {
        return None;
    }
    // SAFETY: a proven, live closure cell; a non-null info is a static one.
    let info = unsafe { (*(ptr as *const ClosureHeader)).info.as_ref()? };
    if info.flags & FN_COMPILED_BODY == 0 || info.flags & NOT_ORDINARY != 0 {
        return None;
    }
    Some(ptr)
}

/// The function's own `prototype` when it holds an ordinary object.
///
/// `prototype` of a function is a non-configurable data property, so the
/// value in the function's own-property bag is authoritative.
///
/// # Safety
/// `closure` is a proven, live closure cell.
#[inline]
unsafe fn own_prototype_value(closure: usize) -> Option<f64> {
    match crate::closure::shape::closure_own_prototype_by_shape(closure as *const ClosureHeader) {
        Some(value) => value,
        None => crate::closure::props::bag_get(closure, b"prototype"),
    }
}

/// [`own_prototype_value`] when it is an ordinary object.
///
/// # Safety
/// `closure` is a proven, live closure cell.
#[inline]
unsafe fn own_prototype_object(closure: usize) -> Option<*mut ObjectHeader> {
    let value = own_prototype_value(closure)?;
    let value = JSValue::from_bits(value.to_bits());
    if !value.is_pointer() {
        return None;
    }
    let proto = value.as_pointer::<ObjectHeader>() as usize;
    let header = crate::value::addr_class::try_read_gc_header(proto)?;
    (header.obj_type == crate::gc::GC_TYPE_OBJECT).then_some(proto as *mut ObjectHeader)
}

/// `proto`'s birth record `(class id, birth ShapeId)`, when one was minted
/// and still names the inline size its class has learned.
///
/// # Safety
/// `proto` is a live `ObjectHeader`.
#[inline]
unsafe fn birth_record(proto: *const ObjectHeader) -> Option<(u32, u32, u32)> {
    let meta = (*proto).meta;
    if meta.is_null() {
        return None;
    }
    let word = (*meta).instance_birth;
    if word == 0 {
        return None;
    }
    let class_id = word as u32;
    let shape_id = (word >> 32) as u32;
    // The birth shape carries the birth live-slot bound, which is the inline
    // size the class has learned; a class that has since learned a larger
    // size gets a new record.
    let slots = crate::object::learned_inline_field_count(class_id);
    // The id is not pinned: the descriptor table may have retired it and
    // handed the id to another shape. Replay it only while it still names
    // the birth facts — keyless, generation 0, no holes, this prototype's
    // identity (so its record's word IS `proto`) and the learned slots.
    crate::object::shapes::shape_is_keyless_birth(shape_id, (*meta).proto_serial, slots)
        .then_some((class_id, shape_id, slots))
}

/// Mint `proto`'s birth record from the first construction, which takes the
/// ordinary sequence: an object of `F`'s synthetic class, linked to `proto`
/// as its class-default prototype. The record is that class id and the
/// ShapeId the link left; [`birth_record`] re-validates the id's facts on
/// every replay, so a retired id is never replayed and a dead function's
/// prototype is not kept alive by a pinned shape. Returns the constructed
/// object.
///
/// The class is the minting function's; a class whose registered prototype is
/// later moved off `proto` clears the record
/// ([`forget_birth_record_of_class`]), so a record never outlives the pairing
/// it was minted from.
#[cold]
#[inline(never)]
unsafe fn mint_birth_record(func_value: f64, proto: *mut ObjectHeader) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let proto_handle = scope.root_raw_mut_ptr(proto);
    let class_id = synthetic_class_id_for_function(func_value);
    let slots = crate::object::learned_inline_field_count(class_id);
    let obj = scope.root_raw_mut_ptr(js_object_alloc(class_id, slots));
    let proto_bits = proto_handle
        .with_mut_ptr::<ObjectHeader, _>(|proto| crate::value::js_nanbox_pointer(proto as i64))
        .to_bits();
    obj.with_mut_ptr::<ObjectHeader, _>(|obj| {
        super::super::super::prototype_chain::object_link_class_default_prototype(
            obj as usize,
            proto_bits,
        )
    });
    let shape_id =
        obj.with_mut_ptr::<ObjectHeader, _>(|obj| crate::object::shapes::object_shape_stamp(obj));
    let meta = proto_handle
        .with_mut_ptr::<ObjectHeader, _>(|proto| crate::object::object_meta_ensure(proto));
    // GC_STORE_AUDIT(POINTER_FREE): a class id and a ShapeId, never a heap
    // reference.
    (*meta).instance_birth = u64::from(class_id) | u64::from(shape_id) << 32;
    obj.with_mut_ptr::<ObjectHeader, _>(|o| crate::value::js_nanbox_pointer(o as i64))
}

/// The class `class_id`'s registered prototype moved from `old` to another
/// object: a birth record on `old` minted for that class no longer describes
/// a construction by its function (#10507).
///
/// # Safety
/// `old` is the live prototype object the registry held for `class_id`.
pub(crate) unsafe fn forget_birth_record_of_class(old: *mut ObjectHeader, class_id: u32) {
    if old.is_null() {
        return;
    }
    let meta = (*old).meta;
    if !meta.is_null() && (*meta).instance_birth as u32 == class_id {
        // GC_STORE_AUDIT(POINTER_FREE): clears a class id / ShapeId pair.
        (*meta).instance_birth = 0;
    }
}

/// An object born from `proto`'s record: class id and birth ShapeId stamped.
/// The ShapeId names `proto` (its record's `prototype` word), which is all
/// the class-default link records; no per-instance record is allocated.
///
/// # Safety
/// `proto` is a live `ObjectHeader` marked as a prototype, and `shape_id`
/// passed [`birth_record`] for it.
#[inline]
unsafe fn born_from_record(class_id: u32, shape_id: u32, slots: u32) -> *mut ObjectHeader {
    crate::object::object_alloc_born(class_id, slots, shape_id)
}

/// `new F(...args)` for an ordinary compiled function `F` (`closure`), or
/// `None` when `F.prototype` is not an ordinary object (the general path
/// handles arrays, functions and primitives there).
///
/// # Safety
/// `closure` is the live closure `func_value` names; `args_ptr` holds
/// `args_len` values.
pub(super) unsafe fn construct_ordinary_compiled_function(
    func_value: f64,
    closure: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let func_handle = scope.root_nanbox_f64(func_value);
    let proto = match own_prototype_object(closure) {
        Some(proto) => proto,
        // Never read: materialize the default prototype the general path
        // would, then read it back off the (possibly moved) function.
        None if own_prototype_value(closure).is_none() => {
            let class_id = synthetic_class_id_for_function(func_value);
            ensure_function_prototype_object(func_value, class_id);
            let closure = (func_handle.get_nanbox_u64() & crate::value::POINTER_MASK) as usize;
            own_prototype_object(closure)?
        }
        None => return None,
    };
    let instance = match birth_record(proto) {
        Some((class_id, shape_id, slots)) => {
            crate::value::js_nanbox_pointer(born_from_record(class_id, shape_id, slots) as i64)
        }
        None => mint_birth_record(func_handle.get_nanbox_f64(), proto),
    };
    Some(run_constructor_body(
        func_handle.get_nanbox_f64(),
        instance,
        args_ptr,
        args_len,
        true,
    ))
}

/// Run `func_value`'s body as a constructor on the fresh `instance`, with
/// `new.target` = `func_value`, and return the construction's result: an
/// object the body returns, else `instance`. `compiled`: `func_value` is an
/// ordinary compiled function ([`ordinary_compiled_function`]), whose body is
/// entered directly rather than through the generic value-call dispatcher.
///
/// # Safety
/// `func_value` is a callable function value; `args_ptr` holds `args_len`
/// values.
pub(super) unsafe fn run_constructor_body(
    func_value: f64,
    instance: f64,
    args_ptr: *const f64,
    args_len: usize,
    compiled: bool,
) -> f64 {
    // #7280: the instance and the two DISPLACED new.target values are held
    // across a call that runs a user constructor body — see the long note in
    // `construct_registered_class_ref`. Unrooted, an evacuating minor moves the
    // instance and this returns the pre-move address.
    let scope = crate::gc::RuntimeHandleScope::new();
    let inst_handle = scope.root_nanbox_f64(instance);
    let prev_new_target = crate::object::js_new_target_get();
    let prev_new_target_handle = scope.root_nanbox_f64(prev_new_target);
    crate::object::js_new_target_set(func_value);
    let prev_current_new_target =
        CURRENT_NEW_TARGET.with(|value| value.replace(func_value.to_bits()));
    let prev_current_new_target_handle = scope.root_nanbox_u64(prev_current_new_target);
    let this = crate::closure::JsThis::from_f64(inst_handle.get_nanbox_f64());
    let result = if compiled {
        let closure = (func_value.to_bits() & crate::value::POINTER_MASK) as *const ClosureHeader;
        crate::closure::call_compiled_closure_this(closure, this, args_ptr, args_len)
    } else {
        crate::closure::native_call_value_this(func_value, this, args_ptr, args_len)
    };
    CURRENT_NEW_TARGET.with(|value| value.set(prev_current_new_target_handle.get_nanbox_u64()));
    crate::object::js_new_target_set(prev_new_target_handle.get_nanbox_f64());
    // Most bodies return `undefined`, which never overrides `this`.
    if result.to_bits() != crate::value::TAG_UNDEFINED && constructor_return_overrides_this(result)
    {
        return result;
    }
    inst_handle.get_nanbox_f64()
}

/// How `value instanceof F` is answered for an ordinary compiled function `F`
/// ([`ordinary_compiled_function_has_instance`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OrdinaryInstanceof {
    /// `value`'s ShapeId names `F.prototype` as its [[Prototype]].
    Instance,
    /// OrdinaryHasInstance's prototype walk is the whole answer: `value` is an
    /// ordinary object of no compiled class, whose [[Prototype]] chain is
    /// exactly what its meta records and shapes say.
    PrototypeWalk,
}

/// `value instanceof F` when `F` is an ordinary compiled function, or `None`
/// for the general path (an `F` that may own `@@hasInstance`, a non-object
/// `F.prototype`, a primitive or exotic `value`, a compiled-class instance
/// whose class chain is not a prototype chain).
///
/// A function-constructed object carries its constructor's synthetic class,
/// but the class is not the answer: `F.prototype` may have been reassigned
/// since (OrdinaryHasInstance compares prototypes, never constructors).
pub(crate) fn ordinary_compiled_function_has_instance(
    value: f64,
    type_ref: f64,
) -> Option<OrdinaryInstanceof> {
    let closure = ordinary_compiled_function(type_ref)?;
    // SAFETY: `closure` is a proven, live closure cell; every pointer below is
    // proven by its GC header before it is read.
    unsafe {
        // InstanceofOperator step 2: an own `@@hasInstance` is a symbol key,
        // which only a FunctionDictionary function object can hold; the
        // inherited one is Function.prototype's, which is non-writable and
        // non-configurable, so OrdinaryHasInstance applies.
        if !crate::closure::shape::closure_on_base_shape(closure as *const ClosureHeader) {
            return None;
        }
        let proto = own_prototype_object(closure)?;
        let bits = value.to_bits();
        if bits & crate::value::TAG_MASK != crate::value::POINTER_TAG {
            return None;
        }
        let obj = (bits & crate::value::POINTER_MASK) as usize;
        let header = crate::value::addr_class::try_read_gc_header(obj)?;
        if header.obj_type != crate::gc::GC_TYPE_OBJECT {
            return None;
        }
        let meta = (*proto).meta;
        if !meta.is_null() && (*meta).proto_serial != 0 {
            let shape_id = crate::object::shapes::object_shape_stamp(obj as *const ObjectHeader);
            if crate::object::shapes::shape_proto_id(shape_id) == Some((*meta).proto_serial) {
                return Some(OrdinaryInstanceof::Instance);
            }
        }
        let class_id = (*(obj as *const ObjectHeader)).class_id;
        (class_id == 0 || class_id >= super::super::prototype_objects::SYNTHETIC_CLASS_ID_BASE)
            .then_some(OrdinaryInstanceof::PrototypeWalk)
    }
}

#[cfg(test)]
#[path = "compiled_function_tests.rs"]
mod tests;
