use perry_ffi::{
    alloc_string, read_string, ArrayHeader, ClosureHeader, JsClosure, JsString, JsValue,
    ObjectHeader, StringHeader, TransientRootScope, TransientRootedNanbox,
};

extern "C" {
    fn js_array_is_array(value: f64) -> f64;
    fn js_date_to_iso_string_or_throw(value: f64) -> *mut StringHeader;
    fn js_get_string_pointer_unified(value: f64) -> i64;
    fn js_jsvalue_to_string(value: f64) -> *mut StringHeader;
    fn js_object_get_field_by_name(
        object: *const ObjectHeader,
        key: *const StringHeader,
    ) -> JsValue;
    fn js_object_keys_value(value: f64) -> *mut ArrayHeader;
    fn js_util_types_is_date(value: f64) -> f64;
    fn js_value_is_closure(value_bits: i64) -> i32;
}

#[inline]
pub(crate) fn as_f64(value: JsValue) -> f64 {
    f64::from_bits(value.bits())
}

#[inline]
pub(crate) fn from_f64(value: f64) -> JsValue {
    JsValue::from_bits(value.to_bits())
}

pub(crate) fn is_array(value: f64) -> bool {
    from_f64(unsafe { js_array_is_array(value) }).to_bool()
}

pub(crate) fn is_date(value: f64) -> bool {
    from_f64(unsafe { js_util_types_is_date(value) }).to_bool()
}

pub(crate) fn is_closure(value: JsValue) -> bool {
    unsafe { js_value_is_closure(value.bits() as i64) != 0 }
}

pub(crate) fn owned_string(scope: &TransientRootScope, value: f64) -> String {
    let rooted = scope.root_nanbox(value);
    let ptr = unsafe { js_jsvalue_to_string(rooted.get()) };
    read_owned_header(ptr)
}

pub(crate) fn string_value(scope: &TransientRootScope, value: f64) -> Option<String> {
    let rooted = scope.root_nanbox(value);
    let js = from_f64(rooted.get());
    if !js.is_any_string() {
        return None;
    }
    let ptr = unsafe { js_get_string_pointer_unified(rooted.get()) } as *mut StringHeader;
    Some(read_owned_header(ptr))
}

pub(crate) fn date_iso(scope: &TransientRootScope, value: f64) -> String {
    let rooted = scope.root_nanbox(value);
    let ptr = unsafe { js_date_to_iso_string_or_throw(rooted.get()) };
    read_owned_header(ptr)
}

pub(crate) fn object_keys(
    scope: &TransientRootScope,
    value: &TransientRootedNanbox,
) -> TransientRootedNanbox {
    let keys = unsafe { js_object_keys_value(value.get()) };
    let boxed = JsValue::from_object_ptr(keys);
    scope.root_nanbox(as_f64(boxed))
}

pub(crate) fn field_by_name(
    scope: &TransientRootScope,
    object: &TransientRootedNanbox,
    name: &str,
) -> JsValue {
    let key = JsValue::from_string_ptr(alloc_string(name).as_raw());
    let key = scope.root_nanbox(as_f64(key));
    field_by_key(object, &key)
}

pub(crate) fn field_by_key(object: &TransientRootedNanbox, key: &TransientRootedNanbox) -> JsValue {
    let key_value = from_f64(key.get());
    let key = if key_value.is_string() {
        key_value.as_string_ptr()
    } else {
        (unsafe { js_get_string_pointer_unified(key.get()) }) as *mut StringHeader
    };
    // Materializing an SSO key may allocate and move the object. Reload the
    // rooted object only after the key is a stable heap string.
    let object = from_f64(object.get()).as_pointer::<ObjectHeader>();
    if object.is_null() || key.is_null() {
        JsValue::UNDEFINED
    } else {
        unsafe { js_object_get_field_by_name(object, key) }
    }
}

pub(crate) fn call1(scope: &TransientRootScope, callback: &TransientRootedNanbox, arg: f64) -> f64 {
    let arg = scope.root_nanbox(arg);
    let callback_value = from_f64(callback.get());
    let closure = unsafe {
        JsClosure::from_raw(callback_value.as_pointer::<ClosureHeader>() as *const ClosureHeader)
    };
    unsafe { closure.call1(arg.get()) }
}

pub(crate) fn call2(
    scope: &TransientRootScope,
    callback: &TransientRootedNanbox,
    arg0: f64,
    arg1: f64,
) -> f64 {
    let arg0 = scope.root_nanbox(arg0);
    let arg1 = scope.root_nanbox(arg1);
    let callback_value = from_f64(callback.get());
    let closure = unsafe {
        JsClosure::from_raw(callback_value.as_pointer::<ClosureHeader>() as *const ClosureHeader)
    };
    unsafe { closure.call2(arg0.get(), arg1.get()) }
}

pub(crate) fn alloc_string_value(value: &str) -> f64 {
    as_f64(JsValue::from_string_ptr(alloc_string(value).as_raw()))
}

fn read_owned_header(ptr: *mut StringHeader) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let string = unsafe { JsString::from_raw(ptr) };
    read_string(string).unwrap_or_default().to_owned()
}
