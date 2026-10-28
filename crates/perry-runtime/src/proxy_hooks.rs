//! Optional Proxy traps. The compiler-selected Proxy installer fills these slots.
use crate::feature_hooks::Hook;
/// The Proxy brand: kind and the cell's exact extent. The extent is only
/// compared once the kind byte matches, so ordinary receivers pay one compare.
#[inline(always)]
pub(crate) fn is_proxy_header(header: &crate::gc::GcHeader) -> bool {
    header.obj_type == crate::gc::GC_TYPE_PROXY
        && header.size as usize
            == crate::gc::GC_HEADER_SIZE + std::mem::size_of::<crate::proxy::ProxyEntry>()
}
#[inline(always)]
pub fn proxy_address(value: f64) -> Option<u64> {
    let bits = value.to_bits();
    let header = crate::value::addr_class::pointer_value_header(bits)?;
    is_proxy_header(header).then_some(bits & crate::value::POINTER_MASK)
}
#[inline(always)]
pub fn proxy_is_proxy(value: f64) -> i32 {
    proxy_address(value).is_some() as i32
}

static PROPERTY_BAG: Hook<fn(usize) -> *mut crate::object::ObjectHeader> = Hook::empty();
#[inline]
pub fn property_bag(owner: usize) -> *mut crate::object::ObjectHeader {
    PROPERTY_BAG
        .get()
        .map_or_else(|| std::ptr::null_mut(), |f| f(owner))
}

static PROPERTY_BAG_ENSURE: Hook<unsafe fn(usize) -> *mut crate::object::ObjectHeader> =
    Hook::empty();
#[inline]
pub unsafe fn property_bag_ensure(owner: usize) -> *mut crate::object::ObjectHeader {
    PROPERTY_BAG_ENSURE
        .get()
        .map_or_else(|| std::ptr::null_mut(), |f| unsafe { f(owner) })
}

static PROXY_WRAPS_CALLABLE: Hook<fn(f64) -> bool> = Hook::empty();
#[inline]
pub fn proxy_wraps_callable(value: f64) -> bool {
    if proxy_address(value).is_none() {
        return crate::object::value_is_callable(value);
    }
    PROXY_WRAPS_CALLABLE
        .get()
        .map_or_else(|| false, |f| f(value))
}

static PROXY_TARGET_CLASS_ID: Hook<fn(f64) -> Option<u32>> = Hook::empty();
#[inline]
pub fn proxy_target_class_id(mut value: f64) -> Option<u32> {
    if proxy_address(value).is_none() {
        if let Some(class_id) = crate::object::class_ref_id(value) {
            return Some(class_id);
        }
        if crate::object::is_class_object_value(value) {
            let bits = value.to_bits();
            let top = bits >> 48;
            let raw = if top == 0x7FFD || top == 0x7FFF || top == (crate::value::HANDLE_TAG >> 48) {
                bits & crate::value::POINTER_MASK
            } else if top == 0 {
                bits
            } else {
                0
            };
            let class_id = crate::object::js_object_get_class_id(raw as *const crate::ObjectHeader);
            return (class_id != 0).then_some(class_id);
        }
        return None;
    }
    PROXY_TARGET_CLASS_ID
        .get()
        .map_or_else(|| None, |f| f(value))
}

static IS_ARRAY_PROXY_STEP: Hook<fn(f64) -> Option<f64>> = Hook::empty();
#[inline]
pub fn is_array_proxy_step(value: f64) -> Option<f64> {
    IS_ARRAY_PROXY_STEP.get().map_or_else(|| None, |f| f(value))
}

static CALL_PROXY_VALUE_WITH_THIS: Hook<fn(f64, f64, &[f64]) -> f64> = Hook::empty();
#[inline]
pub fn call_proxy_value_with_this(proxy: f64, this_arg: f64, args: &[f64]) -> f64 {
    CALL_PROXY_VALUE_WITH_THIS.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(proxy, this_arg, args),
    )
}

static JS_PROXY_GET: Hook<extern "C" fn(f64, f64) -> f64> = Hook::empty();
#[inline]
pub extern "C" fn proxy_get(proxy_boxed: f64, key: f64) -> f64 {
    JS_PROXY_GET.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(proxy_boxed, key),
    )
}

static PROXY_GET_FROM_PROTOTYPE: Hook<fn(f64, f64, f64) -> f64> = Hook::empty();
#[inline]
pub fn proxy_get_from_prototype(proxy_boxed: f64, key: f64, receiver: f64) -> f64 {
    PROXY_GET_FROM_PROTOTYPE.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(proxy_boxed, key, receiver),
    )
}

static PROXY_GET_WITH_RECEIVER: Hook<fn(f64, f64, f64) -> f64> = Hook::empty();
#[inline]
pub fn proxy_get_with_receiver(proxy_boxed: f64, key: f64, receiver: f64) -> f64 {
    PROXY_GET_WITH_RECEIVER.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(proxy_boxed, key, receiver),
    )
}

static JS_PROXY_HAS: Hook<extern "C" fn(f64, f64) -> f64> = Hook::empty();
#[inline]
pub extern "C" fn proxy_has(proxy_boxed: f64, key: f64) -> f64 {
    JS_PROXY_HAS.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(proxy_boxed, key),
    )
}

static JS_PROXY_DELETE: Hook<extern "C" fn(f64, f64) -> f64> = Hook::empty();
#[inline]
pub extern "C" fn proxy_delete(proxy_boxed: f64, key: f64) -> f64 {
    JS_PROXY_DELETE.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(proxy_boxed, key),
    )
}

static JS_PROXY_SET: Hook<extern "C" fn(f64, f64, f64) -> f64> = Hook::empty();
#[inline]
pub extern "C" fn proxy_set(proxy_boxed: f64, key: f64, value: f64) -> f64 {
    JS_PROXY_SET.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(proxy_boxed, key, value),
    )
}

static PROXY_SET_WITH_RECEIVER: Hook<fn(f64, f64, f64, f64) -> f64> = Hook::empty();
#[inline]
pub fn proxy_set_with_receiver(proxy_boxed: f64, key: f64, value: f64, receiver: f64) -> f64 {
    PROXY_SET_WITH_RECEIVER.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(proxy_boxed, key, value, receiver),
    )
}

static JS_PROXY_CHECKED_TARGET: Hook<fn(f64) -> Option<f64>> = Hook::empty();
#[inline]
pub fn proxy_checked_target(proxy_boxed: f64) -> Option<f64> {
    JS_PROXY_CHECKED_TARGET
        .get()
        .map_or_else(|| None, |f| f(proxy_boxed))
}

static JS_PROXY_CHECKED_TARGET_FOR_IS_ARRAY: Hook<fn(f64) -> Option<f64>> = Hook::empty();
#[inline]
pub fn proxy_checked_target_for_is_array(proxy_boxed: f64) -> Option<f64> {
    JS_PROXY_CHECKED_TARGET_FOR_IS_ARRAY
        .get()
        .map_or_else(|| None, |f| f(proxy_boxed))
}

static JS_PROXY_OWN_KEYS_FOR_JSON: Hook<fn(f64) -> f64> = Hook::empty();
#[inline]
pub fn proxy_own_keys_for_json(proxy_boxed: f64) -> f64 {
    JS_PROXY_OWN_KEYS_FOR_JSON.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(proxy_boxed),
    )
}

static JS_PROXY_OWN_KEYS: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
#[inline]
pub extern "C" fn proxy_own_keys(proxy_boxed: f64) -> f64 {
    JS_PROXY_OWN_KEYS.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(proxy_boxed),
    )
}

static PROXY_ENUM_OWN_KEYS: Hook<fn(f64) -> f64> = Hook::empty();
#[inline]
pub fn proxy_enum_own_keys(proxy_boxed: f64) -> f64 {
    PROXY_ENUM_OWN_KEYS.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(proxy_boxed),
    )
}

static PROXY_OWN_PROPERTY_NAMES: Hook<fn(f64) -> f64> = Hook::empty();
#[inline]
pub fn proxy_own_property_names(proxy_boxed: f64) -> f64 {
    PROXY_OWN_PROPERTY_NAMES.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(proxy_boxed),
    )
}

static PROXY_OWN_PROPERTY_SYMBOLS: Hook<fn(f64) -> f64> = Hook::empty();
#[inline]
pub fn proxy_own_property_symbols(proxy_boxed: f64) -> f64 {
    PROXY_OWN_PROPERTY_SYMBOLS.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(proxy_boxed),
    )
}

static PROXY_DEFINE_OWN_PROPERTY_DECODED: Hook<
    unsafe fn(
        &crate::gc::RuntimeHandleScope,
        &crate::gc::RuntimeHandle<'_>,
        &crate::gc::RuntimeHandle<'_>,
        &crate::object::object_ops::DescView<'_>,
    ) -> bool,
> = Hook::empty();
#[inline]
pub unsafe fn proxy_define_own_property_decoded(
    scope: &crate::gc::RuntimeHandleScope,
    receiver: &crate::gc::RuntimeHandle<'_>,
    key: &crate::gc::RuntimeHandle<'_>,
    descriptor: &crate::object::object_ops::DescView<'_>,
) -> bool {
    PROXY_DEFINE_OWN_PROPERTY_DECODED
        .get()
        .map_or_else(|| false, |f| unsafe { f(scope, receiver, key, descriptor) })
}

static JS_PROXY_GET_PROTOTYPE_OF: Hook<fn(f64) -> f64> = Hook::empty();
#[inline]
pub fn proxy_get_prototype_of(obj: f64) -> f64 {
    JS_PROXY_GET_PROTOTYPE_OF
        .get()
        .map_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED), |f| f(obj))
}

static PROXY_GET_PROTOTYPE_OF_IMPL: Hook<fn(f64) -> f64> = Hook::empty();
#[inline]
pub fn proxy_get_prototype_of_impl(obj: f64) -> f64 {
    PROXY_GET_PROTOTYPE_OF_IMPL
        .get()
        .map_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED), |f| f(obj))
}

static PROXY_GET_OWN_PROPERTY_DESCRIPTOR: Hook<
    fn(
        &crate::gc::RuntimeHandleScope,
        &crate::gc::RuntimeHandle<'_>,
        &crate::gc::RuntimeHandle<'_>,
    ) -> f64,
> = Hook::empty();
#[inline]
pub fn proxy_get_own_property_descriptor(
    scope: &crate::gc::RuntimeHandleScope,
    target_handle: &crate::gc::RuntimeHandle<'_>,
    property_key_handle: &crate::gc::RuntimeHandle<'_>,
) -> f64 {
    PROXY_GET_OWN_PROPERTY_DESCRIPTOR.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(scope, target_handle, property_key_handle),
    )
}

static PROXY_IS_EXTENSIBLE: Hook<fn(f64) -> f64> = Hook::empty();
#[inline]
pub fn proxy_is_extensible(target: f64) -> f64 {
    PROXY_IS_EXTENSIBLE.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(target),
    )
}

static PROXY_PREVENT_EXTENSIONS: Hook<fn(f64) -> f64> = Hook::empty();
#[inline]
pub fn proxy_prevent_extensions(target: f64) -> f64 {
    PROXY_PREVENT_EXTENSIONS.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(target),
    )
}

static PROXY_SET_PROTOTYPE_OF: Hook<fn(f64, f64) -> f64> = Hook::empty();
#[inline]
pub fn proxy_set_prototype_of(proxy_boxed: f64, proto: f64) -> f64 {
    PROXY_SET_PROTOTYPE_OF.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(proxy_boxed, proto),
    )
}

static PROXY_IS_CONSTRUCTOR: Hook<fn(f64) -> bool> = Hook::empty();
#[inline]
pub fn proxy_is_constructor(value: f64) -> bool {
    PROXY_IS_CONSTRUCTOR
        .get()
        .map_or_else(|| false, |f| f(value))
}

static JS_PROXY_NEW: Hook<extern "C" fn(f64, f64) -> f64> = Hook::empty();
#[inline]
pub extern "C" fn proxy_new(target: f64, handler: f64) -> f64 {
    JS_PROXY_NEW.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(target, handler),
    )
}

static JS_PROXY_REVOCABLE: Hook<extern "C" fn(f64, f64) -> f64> = Hook::empty();
#[inline]
pub extern "C" fn proxy_revocable(target: f64, handler: f64) -> f64 {
    JS_PROXY_REVOCABLE.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(target, handler),
    )
}

static JS_PROXY_REVOKE: Hook<extern "C" fn(f64)> = Hook::empty();
#[inline]
pub extern "C" fn proxy_revoke(proxy_boxed: f64) {
    JS_PROXY_REVOKE.get().map_or_else(|| (), |f| f(proxy_boxed))
}

static JS_PROXY_IS_REVOKED: Hook<extern "C" fn(f64) -> i32> = Hook::empty();
#[inline]
pub extern "C" fn proxy_is_revoked(proxy_boxed: f64) -> i32 {
    JS_PROXY_IS_REVOKED
        .get()
        .map_or_else(|| 0, |f| f(proxy_boxed))
}

static JS_PROXY_TARGET: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
#[inline]
pub extern "C" fn proxy_target(proxy_boxed: f64) -> f64 {
    JS_PROXY_TARGET.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(proxy_boxed),
    )
}

static JS_PROXY_HANDLER: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
#[inline]
pub extern "C" fn proxy_handler(proxy_boxed: f64) -> f64 {
    JS_PROXY_HANDLER.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(proxy_boxed),
    )
}

static JS_PROXY_APPLY: Hook<extern "C" fn(f64, f64, f64) -> f64> = Hook::empty();
#[inline]
pub extern "C" fn proxy_apply(proxy_boxed: f64, this_arg: f64, args_array: f64) -> f64 {
    JS_PROXY_APPLY.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(proxy_boxed, this_arg, args_array),
    )
}

static JS_PROXY_CONSTRUCT: Hook<extern "C" fn(f64, f64, f64) -> f64> = Hook::empty();
#[inline]
pub extern "C" fn proxy_construct(proxy_boxed: f64, args_array: f64, new_target: f64) -> f64 {
    JS_PROXY_CONSTRUCT.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |f| f(proxy_boxed, args_array, new_target),
    )
}

pub(crate) fn install() {
    PROPERTY_BAG.set(crate::proxy::property_bag);
    PROPERTY_BAG_ENSURE.set(crate::proxy::property_bag_ensure);
    PROXY_WRAPS_CALLABLE.set(crate::proxy::proxy_wraps_callable);
    PROXY_TARGET_CLASS_ID.set(crate::proxy::proxy_target_class_id);
    IS_ARRAY_PROXY_STEP.set(crate::proxy::is_array_proxy_step);
    CALL_PROXY_VALUE_WITH_THIS.set(crate::proxy::apply_construct::call_proxy_value_with_this);
    JS_PROXY_GET.set(crate::proxy::get::js_proxy_get);
    PROXY_GET_FROM_PROTOTYPE.set(crate::proxy::get::proxy_get_from_prototype);
    PROXY_GET_WITH_RECEIVER.set(crate::proxy::get::proxy_get_with_receiver);
    JS_PROXY_HAS.set(crate::proxy::has_delete::js_proxy_has);
    JS_PROXY_DELETE.set(crate::proxy::has_delete::js_proxy_delete);
    JS_PROXY_SET.set(crate::proxy::put_value::js_proxy_set);
    PROXY_SET_WITH_RECEIVER.set(crate::proxy::put_value::proxy_set_with_receiver);
    JS_PROXY_CHECKED_TARGET.set(crate::proxy::json::js_proxy_checked_target);
    JS_PROXY_CHECKED_TARGET_FOR_IS_ARRAY
        .set(crate::proxy::json::js_proxy_checked_target_for_is_array);
    JS_PROXY_OWN_KEYS_FOR_JSON.set(crate::proxy::json::js_proxy_own_keys_for_json);
    JS_PROXY_OWN_KEYS.set(crate::proxy::own_keys::js_proxy_own_keys);
    PROXY_ENUM_OWN_KEYS.set(crate::proxy::own_keys::proxy_enum_own_keys);
    PROXY_OWN_PROPERTY_NAMES.set(crate::proxy::own_keys::proxy_own_property_names);
    PROXY_OWN_PROPERTY_SYMBOLS.set(crate::proxy::own_keys::proxy_own_property_symbols);
    PROXY_DEFINE_OWN_PROPERTY_DECODED
        .set(crate::proxy::reflect_misc::proxy_define_own_property_decoded);
    JS_PROXY_GET_PROTOTYPE_OF.set(crate::proxy::reflect_misc::js_proxy_get_prototype_of);
    PROXY_GET_PROTOTYPE_OF_IMPL.set(crate::proxy::reflect_misc::proxy_get_prototype_of_impl);
    PROXY_GET_OWN_PROPERTY_DESCRIPTOR.set(crate::proxy::reflect::proxy_get_own_property_descriptor);
    PROXY_IS_EXTENSIBLE.set(crate::proxy::reflect_misc::proxy_is_extensible);
    PROXY_PREVENT_EXTENSIONS.set(crate::proxy::reflect_misc::proxy_prevent_extensions);
    PROXY_SET_PROTOTYPE_OF.set(crate::proxy::prototype::proxy_set_prototype_of);
    PROXY_IS_CONSTRUCTOR.set(crate::proxy::proxy_is_constructor);
    JS_PROXY_NEW.set(crate::proxy::js_proxy_new);
    JS_PROXY_REVOCABLE.set(crate::proxy::js_proxy_revocable);
    JS_PROXY_REVOKE.set(crate::proxy::js_proxy_revoke);
    JS_PROXY_IS_REVOKED.set(crate::proxy::js_proxy_is_revoked);
    JS_PROXY_TARGET.set(crate::proxy::js_proxy_target);
    JS_PROXY_HANDLER.set(crate::proxy::js_proxy_handler);
    JS_PROXY_APPLY.set(crate::proxy::apply_construct::js_proxy_apply);
    JS_PROXY_CONSTRUCT.set(crate::proxy::apply_construct::js_proxy_construct);
}

#[inline]
pub(crate) fn canonical_address(value: f64) -> Option<usize> {
    proxy_address(value).map(|p| p as usize)
}
