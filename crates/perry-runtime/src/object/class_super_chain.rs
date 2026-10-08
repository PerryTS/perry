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

use super::class_constructors::pinned_class_object_for_ancestor;

/// The class object of the evaluation a `super` reference in class
/// `home_cid`'s methods belongs to, when one is pinned: repeated evaluations
/// share a template id, so the template tables cannot name it.
#[inline]
pub(crate) fn super_home_owner(home_cid: u32, this_value: f64) -> Option<f64> {
    super::field_get_set::current_private_lexical_brand_value(home_cid).or_else(|| {
        super::field_get_set::private_evaluation_brand_value(this_value)
            .and_then(|owner| pinned_class_object_for_ancestor(owner, home_cid))
    })
}

/// Resolve the home object's current parent for instance and static reads.
/// Repeated evaluations use the method's lexical class owner; the template id
/// alone cannot name their constructor or prototype property storage.
///
/// # Safety
/// `receiver` must be rooted by the caller across prototype materialization.
pub(crate) unsafe fn class_super_base(home_cid: u32, receiver: f64) -> f64 {
    let is_static = super::class_ref_id(receiver).is_some()
        || super::class_registry::is_class_object_value(receiver);
    let scope = crate::gc::RuntimeHandleScope::new();
    let home = match super_home_owner(home_cid, receiver)
        .filter(|owner| super::class_registry::is_class_object_value(*owner))
    {
        Some(owner) => {
            let owner = scope.root_nanbox_f64(owner);
            if is_static {
                owner.get_nanbox_f64()
            } else {
                let obj = crate::value::JSValue::from_bits(owner.get_nanbox_f64().to_bits())
                    .as_pointer::<super::ObjectHeader>();
                f64::from_bits(super::field_get_set::class_object_prototype_value(obj).bits())
            }
        }
        None if is_static => super::class_value::class_value(home_cid),
        None => super::class_registry::class_decl_prototype_value(home_cid),
    };
    super::js_object_get_prototype_of(home)
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
        v.is_pointer()
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
