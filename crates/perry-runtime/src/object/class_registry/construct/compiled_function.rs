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
//! birth ShapeId is the prototype object's birth record
//! (`ObjectMeta::instance_birth`), minted on the first construction by the
//! shape birth allocator and replayed afterwards — the same
//! classless identity and the same ShapeId (its `proto_id` is the prototype's serial,
//! its record's `prototype` word the prototype itself) that sequence
//! produces, with no hash lookup and no per-instance record. A reassigned
//! `F.prototype` is read on the next construction; objects already created
//! keep the prototype their shapes name.
use super::*;

use crate::closure::ClosureHeader;
use std::sync::atomic::{AtomicU64, Ordering};

// Reuse the four words already emitted for every dynamic construct site.
// The high bit distinguishes an ordinary-function entry from a class entry.
const FUNCTION_SITE: u64 = 1 << 63;

/// Admit a previously resolved ordinary body by its immutable info identity.
/// Captures always come from the currently evaluated closure, never the site.
#[inline]
pub(super) unsafe fn ordinary_compiled_function_at_site(
    value: f64,
    site: *const AtomicU64,
) -> Option<usize> {
    if !site.is_null() && (*site).load(Ordering::Relaxed) & FUNCTION_SITE != 0 {
        let bits = value.to_bits();
        if bits & crate::value::TAG_MASK == crate::value::POINTER_TAG {
            let closure = (bits & crate::value::POINTER_MASK) as *const ClosureHeader;
            let header = crate::value::addr_class::try_read_gc_header(closure as usize)?;
            if header.obj_type == crate::gc::GC_TYPE_CLOSURE
                && header.gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
                && ((*closure).shape_id.0 as u64 | FUNCTION_SITE) == (*site).load(Ordering::Relaxed)
                && (*closure).info as u64 == (*site.add(1)).load(Ordering::Relaxed)
            {
                return Some(closure as usize);
            }
        }
    }
    ordinary_compiled_function(value)
}

/// Read the own prototype from a proven bag slot on the current bag shape.
/// Value replacements keep that slot, but are read anew on every construction.
#[inline]
unsafe fn site_prototype_object(
    closure: usize,
    site: *const AtomicU64,
) -> Option<*mut ObjectHeader> {
    if site.is_null()
        || (*site).load(Ordering::Relaxed)
            != (FUNCTION_SITE | (*(closure as *const ClosureHeader)).shape_id.0 as u64)
    {
        return None;
    }
    let proof = (*site.add(2)).load(Ordering::Relaxed);
    let bag = (*(closure as *const ClosureHeader)).props;
    if bag.is_null() || crate::object::shapes::object_shape_stamp(bag) != LayoutWord(proof as u32) {
        return None;
    }
    let slot = (proof >> 32) as u32;
    let prototype_proof = (*site.add(3)).load(Ordering::Relaxed);
    let live = (prototype_proof >> 32) as u32;
    let value = crate::object::object_field_at_with_live(bag, slot, live);
    if !value.is_pointer() {
        return None;
    }
    let proto = value.as_pointer::<ObjectHeader>() as *mut ObjectHeader;
    let header = crate::value::addr_class::try_read_gc_header(proto as usize)?;
    (crate::gc::gc_type_is_object(header.obj_type)
        && crate::object::shapes::object_shape_stamp(proto).0 as u64
            == prototype_proof as u32 as u64)
        .then_some(proto)
}

/// Publish only scalar layout facts and static body identity into the existing site.
unsafe fn prime_function_site(closure: usize, proto: *const ObjectHeader, site: *const AtomicU64) {
    if site.is_null() {
        return;
    }
    let closure = closure as *const ClosureHeader;
    let bag = (*closure).props;
    if bag.is_null() {
        return;
    }
    let shape = crate::object::shapes::object_shape_stamp(bag);
    let Some(record) = crate::object::shapes::shape_record_for_word(shape) else {
        return;
    };
    let Some(Some(slot)) = record.own_data_position_of_value(0, b"prototype") else {
        return;
    };
    (*site.add(1)).store((*closure).info as u64, Ordering::Relaxed);
    (*site.add(2)).store(shape.0 as u64 | (slot as u64) << 32, Ordering::Relaxed);
    (*site.add(3)).store(
        crate::object::shapes::object_shape_stamp(proto).0 as u64
            | (record.live_inline_slot_count() as u64) << 32,
        Ordering::Relaxed,
    );
    (*site).store(
        FUNCTION_SITE | (*closure).shape_id.0 as u64,
        Ordering::Relaxed,
    );
}
use crate::codegen_abi::LayoutWord;
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
    (crate::gc::gc_type_is_object(header.obj_type)).then_some(proto as *mut ObjectHeader)
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
    let meta = crate::object::header::MetaObj::runtime_born(old as *mut _).meta();
    if !meta.is_null() && (*meta).instance_birth as u32 == class_id {
        // GC_STORE_AUDIT(POINTER_FREE): clears a class id / ShapeId pair.
        (*meta).instance_birth = 0;
    }
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
    site: *const AtomicU64,
) -> Option<f64> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let func_handle = scope.root_nanbox_f64(func_value);
    let cached_proto = site_prototype_object(closure, site);
    let proto = match cached_proto.or_else(|| own_prototype_object(closure)) {
        Some(proto) => proto,
        // Never read: materialize the default prototype the general path
        // would, then read it back off the (possibly moved) function.
        None if own_prototype_value(closure).is_none() => {
            ensure_function_prototype_object(func_handle.get_nanbox_f64());
            let closure = (func_handle.get_nanbox_u64() & crate::value::POINTER_MASK) as usize;
            own_prototype_object(closure)?
        }
        None => return None,
    };
    if cached_proto.is_none() {
        prime_function_site(
            (func_handle.get_nanbox_u64() & crate::value::POINTER_MASK) as usize,
            proto,
            site,
        );
    }
    let proto_handle = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(proto as i64));
    // The actual prototype owns the existing shape birth-width family.
    // Marking can collect, so all later pointer reads use the handle.
    let serial = crate::object::proto_validity::mark_object_as_prototype(proto as usize)?;
    let proto = (proto_handle.get_nanbox_u64() & crate::value::POINTER_MASK) as *const ObjectHeader;
    // A native ancestor (`util.inherits(F, http.ServerResponse)`, `Readable`,
    // `EventEmitter`): born on its instance layout's keys with `F.prototype`
    // as prototype, so the ancestor's constructor (`X.call(this)`) fills
    // slots. The chain decides at every birth (one record load per level).
    let recorded = ((*crate::object::header::MetaObj::runtime_born(proto as *mut _).meta())
        .instance_birth
        >> 32) as u32;
    {
        if let Some(layout) = crate::stream_hooks::derived_layout(proto_handle.get_nanbox_u64()) {
            if let Some(obj) = crate::object::layout_desc::alloc_derived_layout_object(
                &*layout,
                serial,
                &proto_handle,
                LayoutWord(recorded),
            ) {
                let shape = crate::object::shapes::object_shape_stamp(obj);
                if shape != LayoutWord(recorded) {
                    let ptr = crate::value::JSValue::from_bits(proto_handle.get_nanbox_u64())
                        .as_pointer::<ObjectHeader>();
                    // GC_STORE_AUDIT(POINTER_FREE): the existing weak class-id-zero / ShapeId pair.
                    (*crate::object::header::MetaObj::runtime_born(ptr as *mut _).meta())
                        .instance_birth = u64::from(shape.0) << 32;
                    crate::object::shapes::note_old_generation_carrier(
                        crate::object::shapes::shape_record_for_word(shape),
                    );
                }
                let instance = crate::value::js_nanbox_pointer(obj as i64);
                return Some(run_constructor_body(
                    func_handle.get_nanbox_f64(),
                    instance,
                    args_ptr,
                    args_len,
                    true,
                ));
            }
        }
    }
    if let Some(obj) = crate::object::function_family_birth::allocate(&func_handle, &proto_handle) {
        return Some(run_constructor_body(
            func_handle.get_nanbox_f64(),
            crate::value::js_nanbox_pointer(obj as i64),
            args_ptr,
            args_len,
            true,
        ));
    }
    let birth = crate::object::shapes::keyless_birth_width(
        serial,
        crate::object::shapes::object_shape_stamp(proto),
    );
    // The existing owner record supplies a scalar shape proof to the same
    // allocator as Object.create. It validates the chosen width and actual
    // prototype serial once, then revalidates after any collecting allocation.
    let recorded = ((*crate::object::header::MetaObj::runtime_born(proto as *mut _).meta())
        .instance_birth
        >> 32) as u32;
    let obj = crate::object::alloc_basic::object_alloc_created(
        &proto_handle,
        serial,
        birth,
        LayoutWord(recorded),
    );
    let shape = crate::object::shapes::object_shape_stamp(obj);
    if shape != LayoutWord(recorded) {
        let ptr = crate::value::JSValue::from_bits(proto_handle.get_nanbox_u64())
            .as_pointer::<ObjectHeader>();
        // GC_STORE_AUDIT(POINTER_FREE): the existing weak class-id-zero / ShapeId pair.
        (*crate::object::header::MetaObj::runtime_born(ptr as *mut _).meta()).instance_birth =
            u64::from(shape.0) << 32;
        crate::object::shapes::note_old_generation_carrier(
            crate::object::shapes::shape_record_for_word(shape),
        );
    }
    let instance = crate::value::js_nanbox_pointer(obj as i64);
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
    let prev_new_target_handle = scope.root_new_target_save(prev_new_target);
    crate::object::js_new_target_set(func_value);
    let prev_current_new_target =
        CURRENT_NEW_TARGET.with(|value| value.replace(func_value.to_bits()));
    let prev_current_new_target_handle = scope.root_nanbox_u64(prev_current_new_target);
    let this = crate::closure::JsThis::from_f64(inst_handle.get_nanbox_f64());
    let result = if compiled {
        let closure = (func_value.to_bits() & crate::value::POINTER_MASK) as *const ClosureHeader;
        crate::closure::call_compiled_body_this(
            closure,
            &*(*closure).info,
            this,
            args_ptr,
            args_len,
        )
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
        if !crate::gc::gc_type_is_object(header.obj_type) {
            return None;
        }
        // A1_UNPROVEN: both: A-tested admission, the meta read needs the B test (kind-A/B question for S0f).
        let meta = crate::object::header::MetaObj::runtime_born(proto as *mut _).meta();
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
