//! ToPrimitive of a Proxy (ECMA-262 7.1.1, 7.1.1.1). A Proxy has no fields of
//! its own: every method lookup is the Proxy's [[Get]] (the `get` trap, or the
//! forward to the target when the handler has none) with the Proxy as the
//! receiver, and every method found is called with `this` = the Proxy.

use crate::value::{POINTER_MASK, POINTER_TAG, TAG_NULL, TAG_UNDEFINED};

/// `ToPrimitive(proxy, hint)`; `hint` is 0 "default", 1 "number", 2 "string"
/// (the `js_to_primitive` encoding). Throws a TypeError where the spec does.
pub(crate) fn proxy_to_primitive(proxy: f64, hint: i32) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let proxy = scope.root_nanbox_f64(proxy);
    // 7.1.1 step 1.a: GetMethod(input, @@toPrimitive).
    let symbol = crate::symbol::well_known_symbol("toPrimitive");
    let symbol = f64::from_bits(POINTER_TAG | (symbol as u64 & POINTER_MASK));
    let exotic =
        crate::proxy::js_reflect_get(proxy.get_nanbox_f64(), symbol, proxy.get_nanbox_f64());
    let bits = exotic.to_bits();
    if bits != TAG_UNDEFINED && bits != TAG_NULL {
        if !crate::proxy_hooks::proxy_wraps_callable(exotic) {
            crate::collection_iter::throw_type_error("Symbol.toPrimitive is not a function");
        }
        let exotic = scope.root_nanbox_f64(exotic);
        let name: &[u8] = match hint {
            1 => b"number",
            2 => b"string",
            _ => b"default",
        };
        let name = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let name = crate::value::js_nanbox_string(name as i64);
        let result = call_with_this(exotic.get_nanbox_f64(), proxy.get_nanbox_f64(), &[name]);
        if crate::value::to_string_primitive::is_primitive_value(result) {
            return result;
        }
        crate::value::to_string_primitive::throw_cannot_convert_to_primitive();
    }
    // 7.1.1 steps 1.b-d: OrdinaryToPrimitive with "default" taken as "number".
    let order: [&[u8]; 2] = if hint == 2 {
        [b"toString", b"valueOf"]
    } else {
        [b"valueOf", b"toString"]
    };
    for name in order {
        let key = crate::value::js_nanbox_string(crate::string::canonical_key(name) as i64);
        let method =
            crate::proxy::js_reflect_get(proxy.get_nanbox_f64(), key, proxy.get_nanbox_f64());
        if !crate::proxy_hooks::proxy_wraps_callable(method) {
            continue;
        }
        let result = call_with_this(method, proxy.get_nanbox_f64(), &[]);
        if crate::value::to_string_primitive::is_primitive_value(result) {
            return result;
        }
    }
    crate::value::to_string_primitive::throw_cannot_convert_to_primitive()
}

/// `Call(method, proxy, args)` for any callable, a callable Proxy included.
fn call_with_this(method: f64, this: f64, args: &[f64]) -> f64 {
    if crate::proxy_hooks::proxy_is_proxy(method) == 1 {
        return crate::proxy_hooks::call_proxy_value_with_this(method, this, args);
    }
    unsafe {
        crate::closure::native_call_value_this(
            method,
            crate::closure::JsThis::from_f64(this),
            args.as_ptr(),
            args.len(),
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::object::ObjectHeader;

    /// The method the get trap hands out: it names its `this`.
    extern "C" fn to_string_names_this(
        _closure: *const crate::closure::ClosureHeader,
        this: crate::closure::JsThis,
    ) -> f64 {
        let text: &[u8] = if crate::proxy_hooks::proxy_is_proxy(this.as_f64()) == 1 {
            b"this=proxy"
        } else {
            b"this=other"
        };
        let s = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
        crate::value::js_nanbox_string(s as i64)
    }

    static METHOD: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    /// `get(target, key, receiver)`: a Symbol key (@@toPrimitive) reads
    /// undefined, every string key reads the method above.
    extern "C" fn get_trap(
        _closure: *const crate::closure::ClosureHeader,
        _this: crate::closure::JsThis,
        _target: f64,
        key: f64,
        _receiver: f64,
    ) -> f64 {
        if unsafe { crate::symbol::js_is_symbol(key) } != 0 {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        f64::from_bits(METHOD.load(std::sync::atomic::Ordering::Relaxed))
    }

    #[test]
    fn string_of_a_proxy_reads_to_string_through_the_get_trap_and_calls_it_on_the_proxy() {
        let _global = crate::gc::global_side_table_test_lock();
        let _no_gc = crate::gc::GcSuppressScope::new();
        crate::proxy_hooks::install();
        unsafe {
            let scope = crate::gc::RuntimeHandleScope::new();
            let object =
                || crate::value::js_nanbox_pointer(crate::object::js_object_alloc(0, 0) as i64);
            let target = scope.root_nanbox_f64(object());
            let handler = scope.root_nanbox_f64(object());
            let method = crate::closure::js_closure_alloc(
                crate::fn_info!(to_string_names_this, 0; with_declared(0)),
                0,
            );
            METHOD.store(
                crate::value::js_nanbox_pointer(method as i64).to_bits(),
                std::sync::atomic::Ordering::Relaxed,
            );
            let trap =
                crate::closure::js_closure_alloc(crate::fn_info!(get_trap, 3; with_declared(3)), 0);
            crate::object::js_object_set_field_by_name(
                crate::value::JSValue::from_bits(handler.get_nanbox_f64().to_bits())
                    .as_pointer::<ObjectHeader>() as *mut ObjectHeader,
                crate::string::canonical_key(b"get"),
                crate::value::js_nanbox_pointer(trap as i64),
            );
            let proxy = scope.root_nanbox_f64(crate::proxy_hooks::proxy_new(
                target.get_nanbox_f64(),
                handler.get_nanbox_f64(),
            ));
            let s = crate::builtins::js_string_coerce(proxy.get_nanbox_f64());
            let len = (*s).byte_len as usize;
            let data = crate::string::string_data(s);
            assert_eq!(
                std::slice::from_raw_parts(data, len),
                b"this=proxy",
                "String(proxy) runs the get trap's toString with this = the Proxy"
            );
        }
    }
}
