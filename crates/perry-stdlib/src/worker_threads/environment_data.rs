use super::*;
use perry_runtime::gc::RuntimeHandleScope;
use perry_runtime::map::{js_map_alloc, js_map_delete, js_map_get, js_map_set, MapHeader};

thread_local! {
    static DATA: Cell<u64> = Cell::new(JSValue::undefined().bits());
}

pub(super) fn scan_roots(visitor: &mut perry_runtime::gc::RuntimeRootVisitor<'_>) {
    DATA.with(|slot| {
        let mut bits = slot.get();
        visitor.visit_nanbox_u64_slot(&mut bits);
        slot.set(bits);
    });
}

fn current() -> f64 {
    f64::from_bits(DATA.with(Cell::get))
}

fn map_ptr(value: f64) -> *mut MapHeader {
    perry_runtime::value::js_nanbox_get_pointer(value) as *mut MapHeader
}

#[no_mangle]
pub extern "C" fn js_worker_threads_get_environment_data(key: f64) -> f64 {
    let map = current();
    if is_undefined(map) {
        js_undefined()
    } else {
        js_map_get(map_ptr(map), key)
    }
}

#[no_mangle]
pub extern "C" fn js_worker_threads_set_environment_data(key: f64, value: f64) -> f64 {
    ensure_environment_data_gc_scanner();
    let scope = RuntimeHandleScope::new();
    let key = scope.root_nanbox_f64(key);
    let value = scope.root_nanbox_f64(value);
    if is_undefined(current()) {
        if is_undefined(value.get_nanbox_f64()) {
            return js_undefined();
        }
        let map = js_map_alloc(0);
        DATA.with(|slot| slot.set(perry_runtime::value::js_nanbox_pointer(map as i64).to_bits()));
    }
    if is_undefined(value.get_nanbox_f64()) {
        js_map_delete(map_ptr(current()), key.get_nanbox_f64());
    } else {
        js_map_set(
            map_ptr(current()),
            key.get_nanbox_f64(),
            value.get_nanbox_f64(),
        );
    }
    js_undefined()
}

pub(super) fn prepare(options: f64) -> SerializedValue {
    let scope = RuntimeHandleScope::new();
    let options = scope.root_nanbox_f64(options);
    let data = scope.root_nanbox_f64(get_object_field_from_value(
        options.get_nanbox_f64(),
        "workerData",
    ));
    let transfer = scope.root_nanbox_f64(get_object_field_from_value(
        options.get_nanbox_f64(),
        "transferList",
    ));
    let payload = perry_runtime::array::js_array_alloc(2);
    let payload = scope.root_nanbox_f64(perry_runtime::value::js_nanbox_pointer(payload as i64));
    let array = || array_ptr_from_value(payload.get_nanbox_f64()).unwrap();
    perry_runtime::array::js_array_push_f64(array(), data.get_nanbox_f64());
    perry_runtime::array::js_array_push_f64(array(), current());
    // One graph preserves data/environment aliases and commits transfers only
    // after both have cloned successfully, in Node's validation order.
    super::clone::clone_worker_data(payload.get_nanbox_f64(), transfer.get_nanbox_f64())
}

pub(super) fn install(payload: SerializedValue) {
    ensure_worker_gc_scanner();
    ensure_environment_data_gc_scanner();
    let scope = RuntimeHandleScope::new();
    let payload = scope.root_nanbox_f64(f64::from_bits(unsafe {
        deserialize_nanbox_on_current_thread(&payload)
    }));
    let array = array_ptr_from_value(payload.get_nanbox_f64()).unwrap();
    let data = perry_runtime::array::js_array_get_f64(array, 0);
    let environment = perry_runtime::array::js_array_get_f64(array, 1);
    CURRENT_WORKER_DATA.with(|slot| *slot.borrow_mut() = Some(data.to_bits()));
    DATA.with(|slot| slot.set(environment.to_bits()));
}

#[cfg(test)]
mod tests {
    use super::*;

    extern "C" {
        fn js_shape_run_static_seed();
    }

    #[test]
    fn environment_map_keeps_key_identity_and_values_across_gc() {
        std::thread::spawn(|| {
            perry_runtime::agent::enter_worker_agent();
            perry_runtime::gc::gc_init();
            unsafe { js_shape_run_static_seed() };
            let scope = RuntimeHandleScope::new();
            let key =
                scope.root_nanbox_f64(object_value(perry_runtime::object::js_object_alloc(0, 0)));
            js_worker_threads_set_environment_data(key.get_nanbox_f64(), 42.0);
            js_worker_threads_set_environment_data(f64::NAN, 7.0);
            js_worker_threads_set_environment_data(-0.0, 8.0);
            perry_runtime::gc::js_gc_collect();
            assert_eq!(
                js_worker_threads_get_environment_data(key.get_nanbox_f64()),
                42.0
            );
            assert!(is_undefined(js_worker_threads_get_environment_data(
                object_value(perry_runtime::object::js_object_alloc(0, 0))
            )));
            assert_eq!(js_worker_threads_get_environment_data(f64::NAN), 7.0);
            assert_eq!(js_worker_threads_get_environment_data(0.0), 8.0);
            js_worker_threads_set_environment_data(key.get_nanbox_f64(), js_undefined());
            assert!(is_undefined(js_worker_threads_get_environment_data(
                key.get_nanbox_f64()
            )));
        })
        .join()
        .unwrap();
    }

    #[test]
    fn snapshot_preserves_worker_data_alias_and_parent_isolation() {
        std::thread::spawn(|| {
            perry_runtime::agent::enter_worker_agent();
            perry_runtime::gc::gc_init();
            unsafe { js_shape_run_static_seed() };
            let scope = RuntimeHandleScope::new();
            let data =
                scope.root_nanbox_f64(object_value(perry_runtime::object::js_object_alloc(0, 0)));
            let options =
                scope.root_nanbox_f64(object_value(perry_runtime::object::js_object_alloc(0, 0)));
            let key = js_string_from_bytes(b"workerData".as_ptr(), 10);
            perry_runtime::object::js_object_set_field_by_name(
                perry_runtime::value::js_nanbox_get_pointer(options.get_nanbox_f64()) as *mut _,
                key,
                data.get_nanbox_f64(),
            );
            js_worker_threads_set_environment_data(string_value("alias"), data.get_nanbox_f64());
            let payload = prepare(options.get_nanbox_f64());
            js_worker_threads_set_environment_data(string_value("alias"), 99.0);
            std::thread::spawn(move || {
                perry_runtime::agent::enter_worker_agent();
                perry_runtime::gc::gc_init();
                install(payload);
                perry_runtime::gc::js_gc_collect();
                let inherited = js_worker_threads_get_environment_data(string_value("alias"));
                assert_eq!(
                    inherited.to_bits(),
                    js_worker_threads_get_worker_data().to_bits()
                );
                js_worker_threads_set_environment_data(string_value("alias"), 77.0);
            })
            .join()
            .unwrap();
            assert_eq!(
                js_worker_threads_get_environment_data(string_value("alias")),
                99.0
            );
        })
        .join()
        .unwrap();
    }
}
