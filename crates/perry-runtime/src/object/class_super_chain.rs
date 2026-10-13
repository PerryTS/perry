//! The live `[[Prototype]]` chain a `super` reference reads.
//!
//! `super.name` is a property lookup on the home object's CURRENT
//! `[[Prototype]]` (the class prototype for an instance member, the class
//! constructor for a static one), with `this` as the receiver. These helpers
//! read that edge and call through the shared accessor lookup. The declared
//! class tables never substitute for the home object's property storage.
//!
//! Split out of `class_constructors.rs` to keep that file under the 2,000-line
//! CI gate.

/// Convert a method's retained lexical class owner to its home object.
/// The caller selects instance versus static; the receiver never participates.
#[no_mangle]
pub unsafe extern "C" fn js_class_method_home(mut owner: f64, is_static: i32) -> f64 {
    // The explicit owner ABI accepts the selected callable or its lexical
    // class. Constructor/class shape identity distinguishes the two; method
    // objects retain their class in capture zero. No receiver/name lookup.
    let value = crate::value::JSValue::from_bits(owner.to_bits());
    let mut constructor = super::class_value::class_value_id(owner);
    if constructor.is_none()
        && value.is_pointer()
        && !super::class_registry::is_class_object_value(owner)
    {
        let ptr = value.as_pointer::<crate::closure::ClosureHeader>();
        if crate::closure::is_closure_ptr(ptr as usize) {
            owner = crate::closure::js_closure_get_capture_f64(ptr, 0);
            constructor = super::class_value::class_value_id(owner);
        }
    }
    if is_static != 0 {
        return owner;
    }
    if let Some(cid) = super::class_prototype_ref_id(owner) {
        return super::class_registry::class_decl_prototype_value(cid);
    }
    if let Some(cid) = constructor {
        return super::class_registry::class_decl_prototype_value(cid);
    }
    let object =
        crate::value::JSValue::from_bits(owner.to_bits()).as_pointer::<super::ObjectHeader>();
    f64::from_bits(super::field_get_set::class_object_prototype_value(object).bits())
}

/// `super.name(...args)` resolved by the shared property lookup: call the value
/// with `this_value` as receiver, or throw the TypeError a call of a
/// non-callable `super.name` throws.
///
/// # Safety
/// `args_ptr` must point to `args_len` valid `f64`s (or be null when
/// `args_len == 0`).
pub(super) unsafe fn super_call_with_lookup(
    key_value: f64,
    this_value: f64,
    args_ptr: *const f64,
    args_len: usize,
    read: impl FnOnce(f64, f64) -> Option<crate::value::JSValue>,
) -> f64 {
    // The read (a getter on the new chain) can collect;
    // the receiver and the arguments ride across them in handles.
    let scope = crate::gc::RuntimeHandleScope::new();
    let this_handle = scope.root_nanbox_f64(this_value);
    let key_handle = scope.root_nanbox_f64(key_value);
    let args: Vec<f64> = if args_len > 0 && !args_ptr.is_null() {
        std::slice::from_raw_parts(args_ptr, args_len).to_vec()
    } else {
        Vec::new()
    };
    let arg_handles = scope.root_nanbox_f64_slice(&args);
    let value = read(key_handle.get_nanbox_f64(), this_handle.get_nanbox_f64());
    let callable = value.filter(|v| {
        let boxed = f64::from_bits(v.bits());
        (v.is_reference())
            && ((crate::proxy::js_proxy_is_proxy(boxed) == 1
                && crate::proxy::proxy_wraps_callable(boxed))
                || crate::closure::is_closure_ptr(
                    crate::value::js_nanbox_get_pointer(boxed) as usize
                ))
    });
    let Some(method) = callable else {
        let hdr = crate::builtins::js_string_coerce(key_handle.get_nanbox_f64());
        let name = super::has_own_helpers::str_from_string_header(hdr).unwrap_or("");
        crate::error::js_throw_type_error_not_a_function(
            std::ptr::null(),
            0,
            name.as_ptr(),
            name.len(),
        )
    };
    let method_handle = scope.root_nanbox_f64(f64::from_bits(method.bits()));
    let args = crate::gc::RuntimeHandleScope::refreshed_nanbox_f64_slice(&arg_handles);
    if crate::proxy::js_proxy_is_proxy(method_handle.get_nanbox_f64()) == 1 {
        return crate::proxy::call_proxy_value_with_this(
            method_handle.get_nanbox_f64(),
            this_handle.get_nanbox_f64(),
            &args,
        );
    }
    crate::closure::native_call_value_this(
        method_handle.get_nanbox_f64(),
        crate::closure::JsThis::from_f64(this_handle.get_nanbox_f64()),
        args.as_ptr(),
        args.len(),
    )
}
