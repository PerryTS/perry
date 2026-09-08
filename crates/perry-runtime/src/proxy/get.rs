//! Proxy [[Get]] carries the original Receiver through traps and target hops.
use super::*;

/// `proxy[key]` uses the proxy itself as Receiver. Prototype and Reflect reads
/// use the explicit-receiver entry below.
#[no_mangle]
pub extern "C" fn js_proxy_get(proxy_boxed: f64, key: f64) -> f64 {
    proxy_get_with_receiver(proxy_boxed, key, proxy_boxed)
}

pub(crate) fn proxy_get_with_receiver(proxy_boxed: f64, key: f64, receiver: f64) -> f64 {
    let _proxy_pin = pin_proxy_for_native_call(proxy_boxed);
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let id = match lookup(proxy_boxed) {
        Some(id) => id,
        None => return f64::from_bits(TAG_UNDEFINED),
    };
    // `[[Get]] ( P, Receiver )` receives an already-computed property key P, but
    // codegen calls this helper with the raw index value for a computed read on
    // a statically-known proxy (`proxy[10]` lowers to
    // `js_proxy_get(proxy, 10.0)`). Apply `ToPropertyKey` so a numeric index is
    // seen by the trap as the canonical string key (`10` -> `"10"`) and the
    // forward-to-target path below stringifies consistently. Symbols and
    // strings pass through unchanged. Without this the get trap received a raw
    // number and key-equality checks (`key === "10"`) silently failed (test262
    // Proxy/get/trap-is-{null,undefined}-target-is-proxy `proxy[10]`). A key
    // that is already a string (the overwhelmingly common `proxy.foo` case) or
    // a symbol is left untouched, so this only pays `ToPropertyKey` for the
    // numeric / object-index forms.
    let key = {
        let tag = key.to_bits() & 0xFFFF_0000_0000_0000;
        let is_string_key =
            tag == crate::value::STRING_TAG || tag == crate::value::SHORT_STRING_TAG;
        if is_string_key || unsafe { crate::symbol::js_is_symbol(key) } != 0 {
            key
        } else {
            unsafe { crate::object::js_to_property_key(key) }
        }
    };
    let (target, handler, revoked) = PROXIES.with(|p| {
        p.borrow()
            .get(id as usize)
            .and_then(|o| o.as_ref())
            .map(|e| (e.target, e.handler, e.revoked))
            .unwrap_or((
                f64::from_bits(TAG_UNDEFINED),
                f64::from_bits(TAG_UNDEFINED),
                false,
            ))
    });
    if revoked {
        return revoked_return();
    }
    // Looking up handler.get can itself run a getter and move the heap.
    let target_h = scope.root_nanbox_f64(target);
    let handler_h = scope.root_nanbox_f64(handler);
    let key_h = scope.root_nanbox_f64(key);
    let trap = handler_trap(handler_h.get_nanbox_f64(), "get");
    if is_callable(trap) {
        let result = call_trap(
            handler_h.get_nanbox_f64(),
            trap,
            &[
                target_h.get_nanbox_f64(),
                key_h.get_nanbox_f64(),
                receiver.get_nanbox_f64(),
            ],
        );
        let result_h = scope.root_nanbox_f64(result);
        invariants::enforce_get_invariant(
            target_h.get_nanbox_f64(),
            key_h.get_nanbox_f64(),
            result_h.get_nanbox_f64(),
        );
        return result_h.get_nanbox_f64();
    }
    // No get trap — forward to the target's `[[Get]]`. A proxy target must
    // recurse through proxy dispatch rather than ordinary target dispatch, which would deref
    // the fake pointer.
    let target = target_h.get_nanbox_f64();
    let key = key_h.get_nanbox_f64();
    if lookup(target).is_some() {
        return proxy_get_with_receiver(target, key, receiver.get_nanbox_f64());
    }
    // `p.apply` / `p.call` / `p.bind` VALUE reads on a callable-wrapping
    // proxy resolve to Function.prototype's methods with the PROXY as the
    // receiver — reify a bound method so a later invocation dispatches
    // `js_native_call_method(proxy, "call", …)` and routes through the
    // proxy's [[Call]] (apply trap). Reading off the target instead would
    // bypass the trap. (Test262 proxy-toString reads `.apply` as a value;
    // Function.prototype.toString on the reified method is the
    // NativeFunction form.)
    if crate::object::value_is_callable(target) {
        if let Some(name) = key_to_rust_string(key) {
            let method: Option<&'static [u8]> = match name.as_str() {
                "apply" => Some(b"apply"),
                "call" => Some(b"call"),
                "bind" => Some(b"bind"),
                _ => None,
            };
            if let Some(m) = method {
                // Only when the target has no OWN override of the slot.
                let t_ptr = extract_pointer(target.to_bits()) as usize;
                if !crate::closure::closure_has_own_dynamic_prop(t_ptr, &name) {
                    return unsafe { crate::closure::reify_function_method_value(proxy_boxed, m) };
                }
            }
        }
    }
    // Ordinary target getters and further prototype hops must keep Receiver.
    // Clear/restore the override around the operation; getters and Proxy hops
    // consume it before entering user code so nested reads bind independently.
    let previous_this = scope.root_nanbox_f64(crate::object::js_implicit_this_set(
        receiver.get_nanbox_f64(),
    ));
    let previous_override =
        crate::object::accessor_receiver_override_begin(receiver.get_nanbox_f64())
            .map(|value| scope.root_nanbox_f64(value));
    let result = target_get_property_key(target_h.get_nanbox_f64(), key_h.get_nanbox_f64());
    crate::object::accessor_receiver_override_end(
        previous_override.map(|value| value.get_nanbox_f64()),
    );
    crate::object::js_implicit_this_set(previous_this.get_nanbox_f64());
    result
}
