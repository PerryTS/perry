use super::*;
use perry_runtime::{gc, object, value};
use std::sync::Mutex;

// Sabotage only: deliberately restore the global owner roots that this family
// removes. This test executable, including this table, never ships.
static SABOTAGE_ROOTS: Mutex<Vec<f64>> = Mutex::new(Vec::new());
fn sabotage_roots(visitor: &mut perry_ffi::GcRootVisitor<'_>) {
    for value in SABOTAGE_ROOTS.lock().unwrap().iter_mut() {
        visitor.visit_nanbox_f64_slot(value);
    }
}
extern "C" fn listener(
    _c: *const perry_runtime::closure::ClosureHeader,
    _this: perry_runtime::closure::JsThis,
) -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}
fn boxed(root: &gc::RuntimeHandle<'_>) -> f64 {
    root.with_mut_ptr(|p: *mut object::ObjectHeader| value::js_nanbox_pointer(p as i64))
}
#[test]
fn response_cycles_are_collected() {
    unsafe {
        super::super::native_dispatch::js_ext_http_nm_install();
    }
    let sabotage = std::env::var_os("PERRY_TEST_RESPONSE_ROOT").is_some();
    if sabotage {
        perry_ffi::gc_register_mutable_root_scanner_named(
            "response-payload-sabotage",
            sabotage_roots,
        );
    }
    let scope = gc::RuntimeHandleScope::new();
    let mut weak = Vec::new();
    for i in 0..64 {
        let holder = {
            let inner = gc::RuntimeHandleScope::new();
            let mut data = ResponseState::new();
            data.standalone = true;
            let response = inner.root_nanbox_f64(value(unsafe { alloc(data) }));
            let cb =
                perry_runtime::closure::js_closure_alloc(perry_runtime::fn_info!(listener, 0), 1);
            let cb = inner.root_raw_mut_ptr(cb);
            cb.with_mut_ptr(|c| {
                perry_runtime::closure::js_closure_set_capture_f64(c, 0, response.get_nanbox_f64())
            });
            let handle = || (response.get_nanbox_f64().to_bits() & PTR_MASK) as i64;
            push(handle(), "once:close", cb.get_raw_mut_ptr::<u8>() as i64);
            // Socket error listener -> response -> socket, plus response listener
            // -> response. Neither cycle has a global root.
            let socket = inner.root_raw_mut_ptr(object::js_object_alloc(0, 0));
            let key = perry_runtime::string::js_string_from_bytes(b"error".as_ptr(), 5);
            object::js_object_set_field_by_name(
                socket.get_raw_mut_ptr(),
                key,
                cb.with_mut_ptr(|c: *mut u8| value::js_nanbox_pointer(c as i64)),
            );
            set(handle(), "socket", boxed(&socket));
            if i % 2 == 0 {
                close(handle());
            }
            if sabotage {
                SABOTAGE_ROOTS
                    .lock()
                    .unwrap()
                    .push(response.get_nanbox_f64());
            }
            let holder = perry_runtime::weakref::js_weakref_new(response.get_nanbox_f64());
            holder
        };
        weak.push(scope.root_nanbox_f64(value::js_nanbox_pointer(holder as i64)));
    }
    gc::js_gc_collect();
    gc::js_gc_collect();
    let live = weak
        .iter()
        .filter(|w| {
            perry_runtime::weakref::js_weakref_deref(w.get_nanbox_f64()).to_bits() != TAG_UNDEFINED
        })
        .count();
    assert_eq!(live, 0, "all response cycles must be collected");
}
#[test]
fn explicit_close_releases_the_payload_but_preserves_metadata() {
    unsafe {
        super::super::native_dispatch::js_ext_http_nm_install();
    }
    let scope = gc::RuntimeHandleScope::new();
    let mut data = ResponseState::new();
    data.standalone = true;
    data.status_code = 201;
    data.headers.insert("x-test".into(), "ok".into());
    let response = scope.root_nanbox_f64(value(unsafe { alloc(data) }));
    let handle = || (response.get_nanbox_f64().to_bits() & PTR_MASK) as i64;
    assert!(unsafe { state(handle()) }.is_some());
    close(handle());
    close(handle());
    assert!(unsafe { state(handle()) }.is_none());
    assert_eq!(closed_property(handle(), "statusCode"), Some(201.0));
    assert_eq!(
        closed_headers(handle())
            .unwrap()
            .get("x-test")
            .map(String::as_str),
        Some("ok")
    );
}
#[test]
fn global_root_sabotage_makes_the_churn_witness_red() {
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "server::response_payload::tests::response_cycles_are_collected",
            "--nocapture",
        ])
        .env("PERRY_TEST_RESPONSE_ROOT", "1")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "sabotage must fail: {stdout}");
    assert!(
        stderr.contains("all response cycles must be collected"),
        "wrong failure: {stdout}\n{stderr}"
    );
}

#[test]
fn every_end_entry_point_releases_the_payload() {
    unsafe {
        super::super::native_dispatch::js_ext_http_nm_install();
    }
    let scope = gc::RuntimeHandleScope::new();
    for kind in 0..3 {
        let mut data = ResponseState::new();
        data.standalone = true;
        let response = scope.root_nanbox_f64(value(unsafe { alloc(data) }));
        let handle = || (response.get_nanbox_f64().to_bits() & PTR_MASK) as i64;
        let undef = f64::from_bits(TAG_UNDEFINED);
        unsafe {
            match kind {
                0 => super::super::response_end::js_node_http_res_end(handle(), undef),
                1 => super::super::response_end::js_node_http_res_end_with_cb(handle(), undef, 0),
                _ => super::super::response_end::js_node_http_res_end_full(handle(), undef, 0, 0),
            }
        }
        assert!(unsafe { state(handle()) }.is_none());
        assert_eq!(
            closed_property(handle(), "writableFinished").map(f64::to_bits),
            Some(boolean(true).to_bits())
        );
    }
}

#[test]
fn destroy_releases_native_state_and_keeps_the_destroyed_flag() {
    unsafe {
        super::super::native_dispatch::js_ext_http_nm_install();
    }
    let scope = gc::RuntimeHandleScope::new();
    let response = scope.root_nanbox_f64(value(unsafe {
        super::super::response::js_node_http_server_response_standalone_new(f64::from_bits(
            TAG_UNDEFINED,
        ))
    }));
    let handle = || (response.get_nanbox_f64().to_bits() & PTR_MASK) as i64;
    unsafe {
        super::super::handle_dispatch::js_ext_http_server_response_dispatch_method(
            handle(),
            b"destroy".as_ptr(),
            7,
            std::ptr::null(),
            0,
        );
    }
    assert!(unsafe { state(handle()) }.is_none());
    assert!(super::super::response::response_destroyed(handle()));
    let destroyed = unsafe {
        super::super::handle_dispatch::js_ext_http_server_response_dispatch_method(
            handle(),
            b"__get_destroyed".as_ptr(),
            15,
            std::ptr::null(),
            0,
        )
    };
    assert_eq!(destroyed.to_bits(), boolean(true).to_bits());
    assert_eq!(
        super::super::response::js_node_http_res_write_with_cb(
            handle(),
            f64::from_bits(TAG_UNDEFINED),
            0
        ),
        0
    );
}

extern "C" fn finish_collects(
    c: *const perry_runtime::closure::ClosureHeader,
    _this: perry_runtime::closure::JsThis,
) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(perry_runtime::closure::js_closure_get_capture_f64(c, 0));
    let handle = || (owner.get().to_bits() & PTR_MASK) as i64;
    assert!(unsafe { state(handle()) }.is_none());
    let count = get(handle(), "listenerCalls");
    set(handle(), "listenerCalls", count + 1.0);
    gc::js_gc_collect();
    f64::from_bits(TAG_UNDEFINED)
}

#[test]
fn end_releases_before_reentrant_listeners_collect() {
    unsafe { super::super::native_dispatch::js_ext_http_nm_install() };
    let scope = gc::RuntimeHandleScope::new();
    let mut data = ResponseState::new();
    data.standalone = true;
    let response = scope.root_nanbox_f64(value(unsafe { alloc(data) }));
    set(
        (response.get_nanbox_f64().to_bits() & PTR_MASK) as i64,
        "listenerCalls",
        0.0,
    );
    for event in ["once:finish", "once:close"] {
        let cb = perry_runtime::closure::js_closure_alloc(
            perry_runtime::fn_info!(finish_collects, 0),
            1,
        );
        let cb = scope.root_raw_mut_ptr(cb);
        cb.with_mut_ptr(|c| {
            perry_runtime::closure::js_closure_set_capture_f64(c, 0, response.get_nanbox_f64())
        });
        push(
            (response.get_nanbox_f64().to_bits() & PTR_MASK) as i64,
            event,
            cb.get_raw_mut_ptr::<u8>() as i64,
        );
    }
    super::super::response_end::js_node_http_res_end(
        (response.get_nanbox_f64().to_bits() & PTR_MASK) as i64,
        f64::from_bits(TAG_UNDEFINED),
    );
    assert!(unsafe { state((response.get_nanbox_f64().to_bits() & PTR_MASK) as i64) }.is_none());
    assert_eq!(
        get(
            (response.get_nanbox_f64().to_bits() & PTR_MASK) as i64,
            "listenerCalls"
        ),
        2.0,
        "both listeners must run, including the snapshot crossing the first collection"
    );
}
