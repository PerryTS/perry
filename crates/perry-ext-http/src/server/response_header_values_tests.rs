//! Original header edges, identity, close and collection, without ambient roots.
use super::header_values as headers;
use super::*;
use perry_runtime::{gc, value, weakref};

fn response(scope: &gc::RuntimeHandleScope) -> gc::RuntimeHandle<'_> {
    unsafe { super::super::native_dispatch::js_ext_http_nm_install() };
    let h = unsafe { js_node_http_server_response_standalone_new(f64::from_bits(TAG_UNDEFINED)) };
    scope.root_nanbox_f64(value::js_nanbox_pointer(h))
}
fn handle(response: &gc::RuntimeHandle<'_>) -> i64 {
    JsValue::from_bits(response.get_nanbox_f64().to_bits()).as_pointer::<u8>() as i64
}
#[test]
fn original_header_array_is_an_owner_edge_after_release() {
    perry_ffi::initialize_runtime_fixture();
    for close_first in [false, true] {
        let scope = gc::RuntimeHandleScope::new();
        let response = response(&scope);
        // Age the owner before attaching the young header array.
        gc::js_gc_collect();
        let weak = {
            let inner = gc::RuntimeHandleScope::new();
            let a = inner.root_raw_mut_ptr(perry_runtime::array::js_array_alloc(1));
            a.with_mut_ptr(|a| perry_runtime::array::js_array_push_f64(a, 23.0));
            let v = a.with_mut_ptr(|a: *mut perry_runtime::array::ArrayHeader| {
                value::js_nanbox_pointer(a as i64)
            });
            headers::set(handle(&response), "X-Test", v);
            let weak = weakref::js_weakref_new(v);
            weak
        };
        let weak = scope.root_nanbox_f64(value::js_nanbox_pointer(weak as i64));
        if close_first {
            heap::close(handle(&response));
        }
        gc::js_gc_collect();
        let original = scope.root_nanbox_f64(headers::lookup(handle(&response), "x-test"));
        assert_ne!(original.get_nanbox_f64().to_bits(), TAG_UNDEFINED);
        assert_eq!(
            weakref::js_weakref_deref(weak.get_nanbox_f64()).to_bits(),
            original.get_nanbox_f64().to_bits()
        );
        assert_eq!(headers::array_get(original.get_nanbox_f64(), 0), 23.0);
        let all = scope.root_nanbox_f64(headers::get_headers(handle(&response)));
        assert_eq!(
            perry_ffi::object_field_by_name(
                JsValue::from_bits(all.get_nanbox_f64().to_bits()),
                "x-test"
            )
            .bits(),
            original.get_nanbox_f64().to_bits()
        );
        headers::remove(handle(&response), "x-test");
        assert!(!headers::has(handle(&response), "x-test"));
    }
}
#[test]
fn response_header_cycles_are_collected_open_and_closed() {
    perry_ffi::initialize_runtime_fixture();
    let scope = gc::RuntimeHandleScope::new();
    let mut weak = Vec::new();
    for closed in [false, true] {
        let w = {
            let inner = gc::RuntimeHandleScope::new();
            let response = response(&inner);
            let a = inner.root_raw_mut_ptr(perry_runtime::array::js_array_alloc(1));
            a.with_mut_ptr(|a| {
                perry_runtime::array::js_array_push_f64(a, response.get_nanbox_f64())
            });
            let a = a.with_mut_ptr(|a: *mut perry_runtime::array::ArrayHeader| {
                value::js_nanbox_pointer(a as i64)
            });
            headers::set(handle(&response), "cycle", a);
            if closed {
                heap::close(handle(&response));
            }
            weakref::js_weakref_new(response.get_nanbox_f64())
        };
        weak.push(scope.root_nanbox_f64(value::js_nanbox_pointer(w as i64)));
    }
    gc::js_gc_collect();
    gc::js_gc_collect();
    for weak in weak {
        assert_eq!(
            weakref::js_weakref_deref(weak.get_nanbox_f64()).to_bits(),
            TAG_UNDEFINED
        );
    }
}
#[test]
fn registry_header_record_is_rewritten_by_the_http_scanner() {
    perry_ffi::initialize_runtime_fixture();
    unsafe { super::super::native_dispatch::js_ext_http_nm_install() };
    let h = register_handle(ServerResponse::new());
    let a = unsafe { perry_ffi::js_array_alloc(0) };
    let original = f64::from_bits(JsValue::from_object_ptr(a).bits());
    headers::set(h, "test", original);
    gc::js_gc_collect();
    assert!(headers::is_array(headers::lookup(h, "test")));
    perry_ffi::drop_handle(h);
}
