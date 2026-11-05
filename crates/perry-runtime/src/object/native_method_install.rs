//! Install native own methods on existing source-compiled receivers.
use crate::closure::js_closure_alloc;
use crate::object::{js_object_set_field_by_name, ObjectHeader};
use crate::value::JSValue;
pub(crate) type StubFn = *const crate::closure::JsFunctionInfo;
/// True when the receiver's class chain declares `name` as a real class method —
/// i.e. the user OVERRODE this native base method (#6316). The class registry is
/// populated at module init, long before any `new`, so the vtable is always
/// live by the time a constructor runs `super()`.
fn class_chain_overrides(class_id: u32, name: &str) -> bool {
    class_id != 0 && crate::object::class_method_slot_owner(class_id, name).is_some()
}

pub(crate) fn install_methods_on_existing_object(
    obj: *mut ObjectHeader,
    this_value: f64,
    methods: &[(&'static str, StubFn)],
    skip_names: &[&str],
) {
    // `js_closure_alloc` and the key interning below both allocate and can
    // therefore GC-move the receiver, so root it and re-read the raw pointer at
    // every use rather than trusting the `obj` snapshot across the loop. The
    // NaN-boxed `this` goes in a handle for the same reason: it is copied into
    // each closure's capture slot, and a stale value there would leave the
    // method bound to a dead receiver. (Captures already stored are traced and
    // rewritten by the GC; a bit-pattern parked in a Rust local is not.)
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = scope.root_raw_mut_ptr(obj);
    let this_handle = scope.root_nanbox_f64(this_value);
    let class_id = crate::object::js_object_get_class_id(obj);

    let mut on_method: Option<crate::gc::RuntimeHandle<'_>> = None;
    for (name, func) in methods {
        if skip_names.iter().any(|skip| skip == name) {
            continue;
        }
        // A native own property would shadow the subclass override.
        // `super` reads the native prototype directly.
        if class_chain_overrides(class_id, name) {
            continue;
        }

        // `addListener` is an ALIAS of `EventEmitter.prototype.on` in Node, so
        // it reuses the base `on` closure even when the subclass overrides
        // `on` — `emitter.addListener(…)` must reach the base, not the override.
        if *name == "addListener" {
            if let Some(val) = &on_method {
                let key = native_method_key(name.as_bytes());
                js_object_set_field_by_name(
                    obj_handle.get_raw_mut_ptr::<ObjectHeader>(),
                    key,
                    val.get_nanbox_f64(),
                );
                continue;
            }
        }
        let closure = js_closure_alloc(*func, 1);
        crate::closure::js_closure_set_capture_ptr(
            closure,
            0,
            this_handle.get_nanbox_f64().to_bits() as i64,
        );
        let val = scope.root_nanbox_f64(f64::from_bits(
            JSValue::pointer(closure as *const u8).bits(),
        ));
        let key = native_method_key(name.as_bytes());
        js_object_set_field_by_name(
            obj_handle.get_raw_mut_ptr::<ObjectHeader>(),
            key,
            val.get_nanbox_f64(),
        );
        if *name == "on" {
            on_method = Some(val);
        }
    }
}

pub(crate) fn native_method_key(bytes: &'static [u8]) -> *mut crate::string::StringHeader {
    let _no_gc = crate::gc::GcSuppressScope::new();
    let hash = crate::object::key_bytes_hash(bytes.as_ptr(), bytes.len());
    #[cfg(test)]
    if std::env::var_os("PERRY_TEST_STREAM_LITERAL_KEY").is_some() {
        return crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    }
    // The canonical-keys trie adopts these same atoms (Appended::atomized).
    // Resolve text identity at its source; traversing a singleton trie list
    // here would add the trie's edge HashMap probe to every construction.
    crate::string::js_string_pool_atom(bytes.as_ptr(), bytes.len() as u32, hash, 0)
}
