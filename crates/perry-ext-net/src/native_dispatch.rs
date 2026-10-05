//! `node:net` exports reached as VALUES (#10429).
//!
//! Direct calls on an import binding (`net.connect(port, host)`) lower through
//! codegen's static native table to this crate's symbols. Every other shape —
//! a CommonJS `require('net')`, an alias (`const n = net`), a pulled-out export
//! (`(0, net_1.createConnection)(opts)`, ioredis/iovalkey), `new` on a bound
//! class value (`const Sock = net.Socket`, pg's function-local require) —
//! reaches perry-runtime's `nm_dispatch_net` bucket instead, which cannot name
//! this crate. The runtime calls back through `js_set_native_net_dispatch`.
//!
//! Registration happens in `js_ext_net_nm_install`, the install symbol codegen
//! emits wherever it materializes a `net` namespace or bound export. It used to
//! be perry-stdlib's http dispatcher, registered only when the stdlib was
//! rebuilt with `external-http-server-pump` — never for a net-only program and
//! never under `PERRY_NO_AUTO_OPTIMIZE=1`, so these forms returned `undefined`.

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

extern "C" {
    fn js_set_native_net_dispatch(
        f: unsafe extern "C" fn(*const u8, usize, *const f64, usize) -> f64,
    );
    fn js_nm_install_net();
    fn js_value_to_str_ptr_for_ffi(value: f64) -> i64;
    fn js_value_is_closure(value_bits: i64) -> i32;
    fn js_net_validate_create_server_options(value: f64);
    fn js_set_tls_connect_provider(f: unsafe extern "C" fn(f64, f64, f64, f64) -> i64);
}

/// Install the runtime's `net` dispatch bucket together with this crate's
/// export dispatcher, so the first value-form call already has a provider.
#[no_mangle]
pub unsafe extern "C" fn js_ext_net_nm_install() {
    js_set_native_net_dispatch(js_ext_net_native_dispatch);
    js_nm_install_net();
    // perry-stdlib's `tls` module dispatch reaches `tls.connect` through this
    // when its archive has no link-time provider (the prebuilt `full` one).
    js_set_tls_connect_provider(crate::tls::js_tls_connect);
}

/// Handle ids box exactly like the static table's `NR_HANDLE_ID` rows; a
/// failed factory (id 0) reads as `undefined`.
fn handle_value(handle: i64) -> f64 {
    if handle == 0 {
        f64::from_bits(TAG_UNDEFINED)
    } else {
        f64::from_bits(POINTER_TAG | (handle as u64 & POINTER_MASK))
    }
}

/// Mirrors `Expr::NetCreateServer`: validate the first argument, then pass the
/// connection listener (the closure argument, in either position) and the
/// options object as raw pointers.
unsafe fn create_server(args_ptr: *const f64, args_len: usize) -> i64 {
    if args_len > 0 {
        js_net_validate_create_server_options(*args_ptr);
    }
    let mut options: i64 = 0;
    let mut listener: i64 = 0;
    for n in 0..args_len.min(2) {
        let bits = (*args_ptr.add(n)).to_bits();
        if js_value_is_closure(bits as i64) != 0 {
            listener = (bits & POINTER_MASK) as i64;
        } else if bits >> 48 == POINTER_TAG >> 48 {
            options = (bits & POINTER_MASK) as i64;
        }
    }
    crate::js_ext_net_create_server(options, listener)
}

/// Runtime callback for `nm_dispatch_net` / `nm_ctor_net`. Each arm calls the
/// same entry point the static native-table row for that export uses, so a
/// value-form socket lands in the same registries as a direct one.
///
/// # Safety
/// `method_ptr`/`method_len` must describe UTF-8 bytes; `args_ptr` must point
/// to `args_len` NaN-boxed values (or be null when `args_len` is 0).
unsafe extern "C" fn js_ext_net_native_dispatch(
    method_ptr: *const u8,
    method_len: usize,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    let undefined = f64::from_bits(TAG_UNDEFINED);
    if method_ptr.is_null() {
        return undefined;
    }
    let method =
        std::str::from_utf8(std::slice::from_raw_parts(method_ptr, method_len)).unwrap_or("");
    // Read slots at each use rather than borrowing a slice: the factories can
    // collect, and a moving collection rewrites rooted slots in place.
    let args_len = if args_ptr.is_null() { 0 } else { args_len };
    let arg = |n: usize| {
        if n < args_len {
            *args_ptr.add(n)
        } else {
            undefined
        }
    };
    match method {
        "connect" | "createConnection" => {
            handle_value(crate::js_ext_net_socket_connect(arg(0), arg(1), arg(2)))
        }
        "createServer" | "Server" => handle_value(create_server(args_ptr, args_len)),
        "Socket" | "Stream" => handle_value(crate::js_net_socket_alloc()),
        "isIP" => crate::ip::js_net_is_ip(js_value_to_str_ptr_for_ffi(arg(0))),
        "isIPv4" => crate::ip::js_net_is_ipv4(js_value_to_str_ptr_for_ffi(arg(0))),
        "isIPv6" => crate::ip::js_net_is_ipv6(js_value_to_str_ptr_for_ffi(arg(0))),
        "getDefaultAutoSelectFamily" => crate::ip::js_net_get_default_auto_select_family(),
        "setDefaultAutoSelectFamily" => crate::ip::js_net_set_default_auto_select_family(arg(0)),
        "getDefaultAutoSelectFamilyAttemptTimeout" => {
            crate::ip::js_net_get_default_auto_select_family_attempt_timeout()
        }
        "setDefaultAutoSelectFamilyAttemptTimeout" => {
            crate::ip::js_net_set_default_auto_select_family_attempt_timeout(arg(0))
        }
        "BlockList.isBlockList" => crate::js_net_block_list_is_block_list(arg(0)),
        "SocketAddress.isSocketAddress" => {
            let bits = arg(0).to_bits();
            let yes = bits & !POINTER_MASK == POINTER_TAG
                && crate::js_ext_net_is_socket_address_handle((bits & POINTER_MASK) as i64) != 0;
            f64::from_bits(if yes {
                0x7FFC_0000_0000_0004
            } else {
                0x7FFC_0000_0000_0003
            })
        }
        "SocketAddress.parse" => crate::js_net_socket_address_parse_value(arg(0)),
        "BlockList" => handle_value(crate::js_net_block_list_new()),
        "SocketAddress" => handle_value(crate::js_net_socket_address_new(arg(0))),
        _ => undefined,
    }
}

#[cfg(test)]
mod statics2_tests {
    use super::*;
    unsafe fn call(name: &str, arg: f64) -> f64 {
        js_ext_net_native_dispatch(name.as_ptr(), name.len(), &arg, 1)
    }
    #[test]
    fn statics2_dispatch_uses_real_net_brands() {
        unsafe {
            let list = call("BlockList", f64::from_bits(TAG_UNDEFINED));
            assert_eq!(
                call("BlockList.isBlockList", list).to_bits(),
                0x7FFC_0000_0000_0004
            );
            assert_eq!(
                call("BlockList.isBlockList", 1.0).to_bits(),
                0x7FFC_0000_0000_0003
            );
            let address = call("SocketAddress", f64::from_bits(TAG_UNDEFINED));
            assert_eq!(
                call("SocketAddress.isSocketAddress", address).to_bits(),
                0x7FFC_0000_0000_0004
            );
            assert_eq!(
                call("SocketAddress.isSocketAddress", list).to_bits(),
                0x7FFC_0000_0000_0003
            );
        }
    }
}

#[cfg(test)]
#[test]
fn statics2_parse_value_and_dispatch_preserve_argument_types() {
    for value in [f64::from_bits(TAG_UNDEFINED), 1.0] {
        assert!(perry_runtime::exception::catch_js_throw(|| unsafe {
            crate::js_net_socket_address_parse_value(value)
        })
        .is_err());
        assert!(perry_runtime::exception::catch_js_throw(|| unsafe {
            js_ext_net_native_dispatch(b"SocketAddress.parse".as_ptr(), 19, &value, 1)
        })
        .is_err());
    }
}
