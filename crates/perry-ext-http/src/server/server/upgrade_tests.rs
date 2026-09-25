//! Exercise the actual pending-upgrade drain and runtime closure boundary.
use super::*;
use perry_ffi::{alloc_closure, closure_capture_f64, set_closure_capture_f64, RawClosureHeader};

extern "C" fn observe(closure: *const RawClosureHeader, req: f64, socket: f64, head: f64) -> f64 {
    unsafe {
        let count = closure_capture_f64(closure, 0);
        set_closure_capture_f64(closure as *mut _, 0, count + 1.0);
        set_closure_capture_f64(closure as *mut _, 1, req);
        set_closure_capture_f64(closure as *mut _, 2, socket);
        let bytes = perry_ffi::read_buffer_bytes(
            (head.to_bits() & PTR_MASK) as *const perry_ffi::BufferHeader,
        )
        .unwrap();
        assert_eq!(bytes, &[0xff, 0, 0x80]);
    }
    0.0
}

fn server(https: bool) -> i64 {
    if https {
        unsafe {
            crate::server::https_server::js_node_https_create_server(
                f64::from_bits(0x7ffc_0000_0000_0001),
                0,
            )
        }
    } else {
        perry_ffi::register_handle(HttpServer::with_handler(0))
    }
}

fn queue(server_handle: i64, request_handle: i64, raw_socket_id: i64) {
    TURNLOOP_UPGRADES
        .lock()
        .unwrap()
        .push_back(HttpPendingUpgrade {
            server_handle,
            request_handle,
            raw_socket_id,
            ws_id: 0,
            head: vec![0xff, 0, 0x80],
        });
}

fn delivery(https: bool) {
    let scope = perry_ffi::TransientRootScope::enter();
    perry_ffi::register_closure_arity(observe as *const u8, 3);
    let callback = scope.root_addr(alloc_closure(observe as *const u8, 3) as i64);
    unsafe {
        set_closure_capture_f64(callback.get() as *mut _, 0, 0.0);
    }
    let server = server(https);
    with_base_server_mut(server, |base| {
        base.once_listeners
            .insert("upgrade".into(), vec![callback.get()]);
    })
    .unwrap();
    let request = perry_ffi::register_handle(String::from("request"));
    let socket = perry_ffi::reserve_handle_id();
    assert!(perry_ext_net::adopt_turnloop_upgrade(socket));
    queue(server, request, socket);
    assert_eq!(drain_upgrades(server), 1);
    unsafe {
        assert_eq!(closure_capture_f64(callback.get() as *const _, 0), 1.0);
        assert_eq!(
            closure_capture_f64(callback.get() as *const _, 1).to_bits(),
            handle_to_pointer_f64(request).to_bits()
        );
        assert_eq!(
            closure_capture_f64(callback.get() as *const _, 2).to_bits(),
            POINTER_TAG | socket as u64
        );
        assert_eq!(
            perry_ext_net::js_net_socket_get_destroyed(socket).to_bits(),
            0x7ffc_0000_0000_0003
        );
    }
    assert!(perry_ffi::get_handle::<String>(request).is_some());
    assert!(with_base_server(server, |base| !base.once_listeners.contains_key("upgrade")).unwrap());
    // A later upgrade has no remaining once listener and must be disposed.
    queue(server, request, socket);
    assert_eq!(drain_upgrades(server), 1);
    unsafe {
        assert_eq!(closure_capture_f64(callback.get() as *const _, 0), 1.0);
        assert_eq!(
            perry_ext_net::js_net_socket_get_destroyed(socket).to_bits(),
            0x7ffc_0000_0000_0004
        );
    }
    assert!(perry_ffi::get_handle::<String>(request).is_none());
    perry_ffi::drop_handle(server);
}

#[test]
fn https_upgrade_delivers_once_then_releases_unclaimed_upgrade() {
    delivery(true);
}

#[test]
fn http_upgrade_delivers_once_then_releases_unclaimed_upgrade() {
    delivery(false);
}

#[test]
fn deleted_server_releases_unclaimed_raw_upgrade() {
    let server = server(true);
    perry_ffi::drop_handle(server);
    let request = perry_ffi::register_handle(String::from("request"));
    let socket = perry_ffi::reserve_handle_id();
    assert!(perry_ext_net::adopt_turnloop_upgrade(socket));
    queue(server, request, socket);
    assert_eq!(drain_upgrades(server), 1);
    assert!(perry_ffi::get_handle::<String>(request).is_none());
    unsafe {
        assert_eq!(
            perry_ext_net::js_net_socket_get_destroyed(socket).to_bits(),
            0x7ffc_0000_0000_0004
        );
    }
}
