//! Sync generator births and their object-owned resume record.
//!
//! The public object has no protocol properties. Its shape proves the private
//! generator brand and its prototype at birth. `meta.native_state` is the
//! already traced GC edge to a single closure whose captures are fixed state
//! slots. No address registry, own-method lookup or const-function finalize.

use super::generator::*;
use super::*;

// Private evaluation brands occupy the low serial range. Reserve a high
// identity for the intrinsic sync-generator brand, carried by all transitions.
pub(crate) const GENERATOR_STATE_BRAND: u64 = u64::MAX - 2;

/// A fresh class evaluation owns the actual functions of its symbol methods.
/// Their keys are retained in its existing computed-declaration slots.
pub(crate) unsafe fn install_evaluation_generator_symbols(proto: f64, owner: f64, class_id: u32) {
    let Some(decl) = crate::object::class_registry::declarations::class_declaration(class_id)
    else {
        return;
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let proto = scope.root_nanbox_f64(proto);
    let owner = scope.root_nanbox_f64(owner);
    for member in decl
        .members()
        .iter()
        .filter(|m| m.name.is_null() && m.is_method() && !m.entry.is_null())
    {
        if !crate::closure::info_has(
            &*(member.entry as *const crate::closure::JsFunctionInfo),
            crate::closure::FN_GENERATOR,
        ) {
            continue;
        }
        let Some(key) = crate::object::class_registry::declarations::evaluation_computed_member_key(
            crate::value::js_nanbox_get_pointer(owner.get_nanbox_f64()) as *const ObjectHeader,
            class_id,
            member,
        ) else {
            continue;
        };
        if crate::symbol::js_is_symbol(key) == 0 {
            continue;
        }
        let key = scope.root_nanbox_f64(key);
        let name = crate::symbol::symbol_function_name(crate::symbol::sym_key_from_f64(
            key.get_nanbox_f64(),
        ));
        let method = scope.root_nanbox_f64(
            crate::object::native_module::class_method_declaration_value(
                owner.get_nanbox_f64(),
                &name,
                member.code as usize,
                member.param_count,
                member.has_synthetic_arguments(),
                member.has_rest(),
                member.entry as usize,
                true,
            ),
        );
        crate::object::native_module::set_bound_native_closure_name(
            crate::value::js_nanbox_get_pointer(method.get_nanbox_f64())
                as *mut crate::closure::ClosureHeader,
            &name,
        );
        crate::symbol::define_symbol_data_property(
            proto.get_nanbox_f64(),
            key.get_nanbox_f64(),
            method.get_nanbox_f64(),
        );
        crate::symbol::set_symbol_property_attrs(
            crate::value::js_nanbox_get_pointer(proto.get_nanbox_f64()) as usize,
            crate::symbol::sym_key_from_f64(key.get_nanbox_f64()),
            crate::object::PropertyAttrs::new(true, false, true),
        );
    }
}

/// A computed static symbol method is an ordinary own property of its class.
/// Retain its actual callable and home, as named class methods already do.
pub(crate) unsafe fn install_class_symbol_method(
    class_id: u32,
    owner: f64,
    key: f64,
    body: usize,
    params: u32,
    rest: bool,
    entry: usize,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let key = scope.root_nanbox_f64(key);
    let owner = if crate::object::value_is_object_like(owner) {
        owner
    } else {
        crate::object::class_value::class_value(class_id)
    };
    let owner = scope.root_nanbox_f64(owner);
    let name =
        crate::symbol::symbol_function_name(crate::symbol::sym_key_from_f64(key.get_nanbox_f64()));
    // Use the same declaration value as named/computed-string methods:
    // its home and ordinary method metadata belong to this callable.
    let method = crate::object::native_module::class_method_declaration_value(
        owner.get_nanbox_f64(),
        name.as_bytes(),
        body,
        params,
        false,
        rest,
        entry,
        true,
    );
    let method = scope.root_nanbox_f64(method);
    crate::symbol::define_symbol_data_property(
        owner.get_nanbox_f64(),
        key.get_nanbox_f64(),
        method.get_nanbox_f64(),
    );
    crate::symbol::set_symbol_property_attrs(
        crate::value::js_nanbox_get_pointer(owner.get_nanbox_f64()) as usize,
        crate::symbol::sym_key_from_f64(key.get_nanbox_f64()),
        crate::object::PropertyAttrs::new(true, false, true),
    );
}

/// The source entry selects generator lowering, while Get and the ordinary
/// callable invocation prove the selected method and its receiver per use.
pub(crate) unsafe fn call_generator_symbol_method(
    object: f64,
    key: f64,
    args: *const f64,
    len: usize,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let object = scope.root_nanbox_f64(object);
    let key = scope.root_nanbox_f64(key);
    let args = scope.root_nanbox_f64_slice(if len == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(args, len)
    });
    let recv = if let Some(cid) = crate::object::class_prototype_ref_id(object.get_nanbox_f64()) {
        crate::object::class_registry::class_decl_prototype_value(cid)
    } else if let Some(cid) =
        crate::object::class_value::legacy_class_value_word(object.get_nanbox_u64())
    {
        crate::value::js_nanbox_pointer(crate::object::class_value::class_value_ptr(cid) as i64)
    } else {
        object.get_nanbox_f64()
    };
    let recv = scope.root_nanbox_f64(recv);
    let value =
        crate::symbol::js_object_get_symbol_property(recv.get_nanbox_f64(), key.get_nanbox_f64());
    let args = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&args);
    crate::object::method_call::js_method_call_value(
        value,
        recv.get_nanbox_f64(),
        args.as_ptr(),
        args.len(),
    )
}

pub(crate) fn fixed_generator_state(value: f64) -> Option<*const crate::closure::ClosureHeader> {
    let value = JSValue::from_bits(value.to_bits());
    if !value.is_pointer() {
        return None;
    }
    let addr = value.as_pointer::<u8>() as usize;
    unsafe {
        let header = crate::value::addr_class::try_read_gc_header(addr)?;
        if header.obj_type != crate::gc::GC_TYPE_OBJECT {
            return None;
        }
        let obj = addr as *const ObjectHeader;
        let meta = (*obj).meta;
        if meta.is_null() {
            return None;
        }
        let state = (*meta).native_state;
        if !crate::native_payload::is_payload_state_word(state) {
            return None;
        }
        if !crate::object::shapes::object_has_brand(obj, GENERATOR_STATE_BRAND) {
            return None;
        }
        Some((state & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader)
    }
}

/// The generator birth owns an inaccessible, two-parameter compiled resume
/// body (one argument for bodies with no abrupt routes). Once the receiver's brand proves that edge, no general callable,
/// rest-argument or arity dispatch remains to resolve.
#[inline(never)]
pub(super) unsafe fn resume_fixed_state(
    state: *const crate::closure::ClosureHeader,
    op: f64,
    arg: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let state = scope.root_raw_const_ptr(state);
    let get = |index| {
        state.with_const_ptr::<crate::closure::ClosureHeader, _>(|p| {
            *p.add(1).cast::<u64>().add(index)
        })
    };
    let set = |index, bits| {
        state.with_const_ptr::<crate::closure::ClosureHeader, _>(|p| {
            // GC_STORE_AUDIT(POINTER_FREE): shared done/executing controls only.
            p.add(1).cast::<u64>().cast_mut().add(index).write(bits);
        })
    };
    if get(3) == crate::value::TAG_TRUE {
        let msg = b"Generator is already executing";
        let msg = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        let err = crate::error::js_typeerror_new(msg);
        crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64));
    }
    let next_only =
        state.with_const_ptr::<crate::closure::ClosureHeader, _>(|p| (*(*p).info).params == 1);
    if get(1) == crate::value::TAG_TRUE || (op != 0.0 && (next_only || get(0) == 0.0f64.to_bits()))
    {
        set(1, crate::value::TAG_TRUE);
        if op == 2.0 {
            crate::exception::js_throw(arg);
        }
        let value = if op == 1.0 {
            arg
        } else {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        };
        return js_generator_result(value, 1);
    }
    set(3, crate::value::TAG_TRUE);
    if op == 0.0 {
        state.with_const_ptr::<crate::closure::ClosureHeader, _>(|p| {
            js_generator_set_slot(p.cast_mut(), 2, arg.to_bits(), 1)
        });
    }
    // next consumes the sent slot from this refreshed frame at body entry,
    // including after barrier work. Abrupt arguments cross no slot barrier;
    // their remaining setup is the non-collecting exception-stack capture.
    // The argument is never read here after the body can collect.
    let result = crate::exception::catch_generator_resume(&state, op, arg);
    set(3, crate::value::TAG_FALSE);
    match result {
        Ok(value) => value,
        Err(value) => {
            set(1, crate::value::TAG_TRUE);
            crate::exception::js_throw(value)
        }
    }
}

/// A previously marked prototype already owns its stable serial. Read those
/// existing metadata facts before asking the general allocating mark path.
unsafe fn prototype_identity(value: f64) -> Option<u64> {
    let addr = (value.to_bits() & crate::value::POINTER_MASK) as usize;
    if let Some(header) = crate::value::addr_class::try_read_gc_header(addr) {
        if header.obj_type == crate::gc::GC_TYPE_OBJECT {
            let meta = (*(addr as *const ObjectHeader)).meta;
            if !meta.is_null()
                && (*meta).flags & crate::object::OBJECT_META_FLAG_IS_PROTOTYPE != 0
                && (*meta).proto_serial != 0
            {
                return Some((*meta).proto_serial);
            }
        }
    }
    crate::object::proto_validity::mark_object_as_prototype(addr)
}

#[no_mangle]
pub extern "C" fn js_generator_create(
    resume: f64,
    owner: *const crate::closure::ClosureHeader,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let resume = scope.root_nanbox_f64(resume);
    // Direct class-method bodies have no incoming closure parameter. The
    // image identity of their private resume body supplies the function's
    // ordinary prototype property, rather than an intermediate per birth.
    let owner = if owner.is_null() {
        let state = crate::value::js_nanbox_get_pointer(resume.get_nanbox_f64())
            as *const crate::closure::ClosureHeader;
        unsafe { crate::closure::js_closure_alloc_singleton((*state).info) }
    } else {
        owner as *mut crate::closure::ClosureHeader
    };
    let owner = scope.root_raw_const_ptr(owner);
    let proto = owner.with_const_ptr::<crate::closure::ClosureHeader, _>(|owner| {
        if owner.is_null() {
            None
        } else {
            generator_function_prototype_of(owner as usize)
        }
    });
    // GetPrototypeFromConstructor: a replaced primitive `.prototype` selects
    // the realm's intrinsic prototype. Object values keep their exact identity.
    let proto = proto
        .filter(|value| unsafe { crate::object::value_is_object_like(*value) })
        .unwrap_or_else(|| crate::value::js_nanbox_pointer(generator_prototype_ptr(false) as i64));
    let proto = scope.root_nanbox_f64(proto);
    let proto_id = unsafe { prototype_identity(proto.get_nanbox_f64()) }
        .unwrap_or_else(crate::object::shapes::fresh_unique_proto_id);

    // Serial descriptors snapshot their link at mint time. Publish that link
    // first so the born shape itself proves the inherited protocol chain.
    if crate::object::shapes::identity_prototype_word(proto_id) != proto.get_nanbox_u64() {
        crate::object::shapes::write_identity_word(proto_id, proto.get_nanbox_u64());
    }

    let obj = scope.root_raw_mut_ptr(crate::object::alloc_basic::object_alloc_unpublished(0, 0));
    let shape = crate::object::shapes::publish_shape_result(
        crate::object::shapes::shape_descriptor_intern_with_rep(
            std::ptr::null(),
            0,
            0,
            0,
            crate::object::shapes::ShapeObjectKind::Ordinary,
            0,
            proto_id,
            0,
            crate::object::field_rep::REP_ANY,
            &[GENERATOR_STATE_BRAND],
            None,
            0, // newborn generator is extensible, independent of its prototype
        ),
    );
    // The shape owns the prototype edge. The final descriptor and its brand
    // are published together, before metadata allocation can collect.
    obj.with_mut_ptr::<ObjectHeader, _>(|obj| unsafe {
        crate::object::shapes::store_kind::premark_plain_ordinary(obj);
        crate::object::shapes::stamp_object_shape_id_with_carrier_note(obj, shape);
    });
    let (meta, _) = obj.across_mut::<ObjectHeader, _>(|| {
        obj.with_mut_ptr::<ObjectHeader, _>(|obj| unsafe { crate::object::object_meta_ensure(obj) })
    });
    unsafe {
        let state = resume.get_nanbox_u64();
        // GC_STORE_AUDIT(BARRIERED): native_state is a tagged child edge.
        (*meta).native_state = state;
        crate::gc::runtime_write_barrier_slot(
            meta as usize,
            &(*meta).native_state as *const _ as usize,
            state,
        );
        let state = (state & crate::value::POINTER_MASK) as *mut crate::closure::ClosureHeader;
        debug_assert!(matches!((*(*state).info).params, 1 | 2));
        debug_assert_eq!((*(*state).info).flags & crate::closure::FN_REST_MASK, 0);
        // A fixed generator frame contains JSValues throughout its lifetime.
        // Use the existing tag scanner even when its initial values happen to
        // be scalar. Subsequent writes need only the exact write barrier,
        // rather than per-operation dynamic capture-mask maintenance.
        crate::gc::layout_mark_unknown(state.cast());
        if (*state).capture_count & crate::closure::CAPTURES_THIS_FLAG != 0 {
            // GC_STORE_AUDIT(POINTER_FREE): lexical receiver is never rebound.
            (*state).capture_count |= crate::closure::NO_THIS_REBIND_FLAG;
        }
    }
    crate::gc::runtime_shade_external_edge(proto.get_nanbox_u64());
    obj.with_mut_ptr::<ObjectHeader, _>(|obj| crate::value::js_nanbox_pointer(obj as i64))
}

/// Fixed-frame stores share one barrier path. The birth selected the generic
/// JSValue scanner; no capture-layout update or callable check is needed.
#[no_mangle]
#[inline(never)]
pub unsafe extern "C" fn js_generator_set_slot(
    state: *mut crate::closure::ClosureHeader,
    index: u32,
    bits: u64,
    string_alias: i32,
) {
    // Preserve binding-alias demotion in this shared operation. Fresh string
    // computations and an accumulator's own update retain their append proof.
    if string_alias != 0 && bits & crate::value::TAG_MASK == crate::value::STRING_TAG {
        crate::string::js_string_addref(
            (bits & crate::value::POINTER_MASK) as *mut crate::StringHeader,
        );
    }
    let slot = state.add(1).cast::<u64>().add(index as usize);
    // GC_STORE_AUDIT(BARRIERED): exact fixed-frame destination below.
    slot.write(bits);
    crate::gc::runtime_write_barrier_slot(state as usize, slot as usize, bits);
}

/// Public result construction reuses the runtime's canonical iterator shape.
#[no_mangle]
#[inline(never)]
pub extern "C" fn js_generator_result(value: f64, done: i32) -> f64 {
    unsafe { crate::iter_result::make_iter_result(JSValue::from_bits(value.to_bits()), done != 0) }
}
