use super::*;

use crate::value::{js_nanbox_pointer, JSValue};

#[no_mangle]
pub extern "C" fn js_dns_noop(_args: i64) -> f64 {
    undefined_value()
}

#[no_mangle]
pub extern "C" fn js_dns_lookup(args: i64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let hostname_value = arg(args, 0);
    let hostname = match js_string_to_rust(hostname_value) {
        Some(hostname) => hostname,
        None => throw_error_value(invalid_hostname_error(hostname_value)),
    };

    let second = arg(args, 1);
    let (options_value, callback_value) = if is_callable_value(second) {
        (undefined_value(), second)
    } else {
        (second, arg(args, 2))
    };
    let callback_handle = scope.root_nanbox_f64(callback_value);
    if !is_callable_value(callback_value) {
        throw_error_value(invalid_callback_error(callback_value));
    }

    let options = match parse_lookup_options(options_value) {
        Ok(options) => options,
        Err(error) => throw_error_value(error),
    };
    let callback_args = match lookup_callback_values(&hostname, options) {
        Ok(values) => values,
        Err(error) => vec![error],
    };
    queue_callback(
        callback_handle.get_nanbox_f64(),
        &callback_args,
        "GETADDRINFOREQWRAP",
    );
    undefined_value()
}

#[no_mangle]
pub extern "C" fn js_dns_lookup_service(args: i64) -> f64 {
    if args_len(args) < 3 || JSValue::from_bits(arg(args, 2).to_bits()).is_undefined() {
        throw_error_value(lookup_service_missing_args_error());
    }

    let address_value = arg(args, 0);
    let address = match js_string_to_rust(address_value) {
        Some(address) => address,
        None => throw_error_value(invalid_address_error(address_value)),
    };
    let port = match parse_lookup_service_port(arg(args, 1)) {
        Ok(port) => port,
        Err(error) => throw_error_value(error),
    };
    let callback_value = arg(args, 2);
    if !is_callable_value(callback_value) {
        throw_error_value(invalid_callback_error(callback_value));
    }
    let callback_args = match lookup_service_result(&address, port) {
        Ok((hostname, service)) => vec![null_value(), str_value(&hostname), str_value(&service)],
        Err(error) => vec![error],
    };
    queue_callback(callback_value, &callback_args, "GETNAMEINFOREQWRAP");
    undefined_value()
}

#[no_mangle]
pub extern "C" fn js_dns_resolve(args: i64) -> f64 {
    dns_callback_resolve(args, None)
}

#[no_mangle]
pub extern "C" fn js_dns_resolve4(args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::A))
}

#[no_mangle]
pub extern "C" fn js_dns_resolve6(args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Aaaa))
}

#[no_mangle]
pub extern "C" fn js_dns_resolve_any(args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Any))
}

#[no_mangle]
pub extern "C" fn js_dns_resolve_caa(args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Caa))
}

#[no_mangle]
pub extern "C" fn js_dns_resolve_cname(args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Cname))
}

#[no_mangle]
pub extern "C" fn js_dns_resolve_mx(args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Mx))
}

#[no_mangle]
pub extern "C" fn js_dns_resolve_naptr(args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Naptr))
}

#[no_mangle]
pub extern "C" fn js_dns_resolve_ns(args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Ns))
}

#[no_mangle]
pub extern "C" fn js_dns_resolve_ptr(args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Ptr))
}

#[no_mangle]
pub extern "C" fn js_dns_resolve_soa(args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Soa))
}

#[no_mangle]
pub extern "C" fn js_dns_resolve_srv(args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Srv))
}

#[no_mangle]
pub extern "C" fn js_dns_resolve_tlsa(args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Tlsa))
}

#[no_mangle]
pub extern "C" fn js_dns_resolve_txt(args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Txt))
}

#[no_mangle]
pub extern "C" fn js_dns_reverse(args: i64) -> f64 {
    dns_callback_reverse(args)
}

#[no_mangle]
pub extern "C" fn js_dns_promises_noop(_args: i64) -> f64 {
    let promise = crate::promise::js_promise_resolved(undefined_value());
    js_nanbox_pointer(promise as i64)
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolve(args: i64) -> f64 {
    dns_promise_resolve(args, None)
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolve4(args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::A))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolve6(args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Aaaa))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolve_any(args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Any))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolve_caa(args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Caa))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolve_cname(args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Cname))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolve_mx(args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Mx))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolve_naptr(args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Naptr))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolve_ns(args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Ns))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolve_ptr(args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Ptr))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolve_soa(args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Soa))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolve_srv(args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Srv))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolve_tlsa(args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Tlsa))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolve_txt(args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Txt))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_reverse(args: i64) -> f64 {
    dns_promise_reverse(args)
}

#[no_mangle]
pub extern "C" fn js_dns_get_servers(_args: i64) -> f64 {
    dns_get_servers_value()
}

#[no_mangle]
pub extern "C" fn js_dns_set_servers(args: i64) -> f64 {
    dns_set_servers_value(first_arg(args))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_get_servers(_args: i64) -> f64 {
    dns_promises_get_servers_value()
}

#[no_mangle]
pub extern "C" fn js_dns_promises_set_servers(args: i64) -> f64 {
    dns_promises_set_servers_value(first_arg(args))
}

#[no_mangle]
pub extern "C" fn js_dns_set_default_result_order(args: i64) -> f64 {
    dns_set_default_result_order_value(first_arg(args))
}

#[no_mangle]
pub extern "C" fn js_dns_get_default_result_order(_args: i64) -> f64 {
    dns_get_default_result_order_value()
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_new(args: i64) -> f64 {
    boxed_pointer(resolver_object(stored_servers(), first_arg(args)) as *const u8)
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolver_new(args: i64) -> f64 {
    boxed_pointer(resolver_object(stored_promise_servers(), first_arg(args)) as *const u8)
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_get_servers(_handle: i64, _args: i64) -> f64 {
    let Some(obj) = resolver_object_from_handle(_handle) else {
        return empty_array_value();
    };
    resolver_get_servers_from_obj(obj)
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_set_servers(handle: i64, args: i64) -> f64 {
    let servers_value = first_arg(args);
    let Some(obj) = resolver_object_from_handle(handle) else {
        return dns_promises_set_servers_value(servers_value);
    };
    resolver_set_servers_for_obj(obj, servers_value)
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_noop(_handle: i64, _args: i64) -> f64 {
    undefined_value()
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_resolve(_handle: i64, args: i64) -> f64 {
    dns_callback_resolve(args, None)
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_resolve4(_handle: i64, args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::A))
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_resolve6(_handle: i64, args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Aaaa))
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_resolve_any(_handle: i64, args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Any))
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_resolve_caa(_handle: i64, args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Caa))
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_resolve_cname(_handle: i64, args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Cname))
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_resolve_mx(_handle: i64, args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Mx))
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_resolve_naptr(_handle: i64, args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Naptr))
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_resolve_ns(_handle: i64, args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Ns))
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_resolve_ptr(_handle: i64, args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Ptr))
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_resolve_soa(_handle: i64, args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Soa))
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_resolve_srv(_handle: i64, args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Srv))
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_resolve_tlsa(_handle: i64, args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Tlsa))
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_resolve_txt(_handle: i64, args: i64) -> f64 {
    dns_callback_resolve(args, Some(RecordKind::Txt))
}

#[no_mangle]
pub extern "C" fn js_dns_resolver_reverse(_handle: i64, args: i64) -> f64 {
    dns_callback_reverse(args)
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolver_resolve(_handle: i64, args: i64) -> f64 {
    dns_promise_resolve(args, None)
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolver_resolve4(_handle: i64, args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::A))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolver_resolve6(_handle: i64, args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Aaaa))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolver_resolve_any(_handle: i64, args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Any))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolver_resolve_caa(_handle: i64, args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Caa))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolver_resolve_cname(_handle: i64, args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Cname))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolver_resolve_mx(_handle: i64, args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Mx))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolver_resolve_naptr(_handle: i64, args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Naptr))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolver_resolve_ns(_handle: i64, args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Ns))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolver_resolve_ptr(_handle: i64, args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Ptr))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolver_resolve_soa(_handle: i64, args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Soa))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolver_resolve_srv(_handle: i64, args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Srv))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolver_resolve_tlsa(_handle: i64, args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Tlsa))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolver_resolve_txt(_handle: i64, args: i64) -> f64 {
    dns_promise_resolve(args, Some(RecordKind::Txt))
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolver_reverse(_handle: i64, args: i64) -> f64 {
    dns_promise_reverse(args)
}

#[no_mangle]
pub extern "C" fn js_dns_promises_lookup(args: i64) -> f64 {
    let hostname_value = arg(args, 0);
    let hostname_js = JSValue::from_bits(hostname_value.to_bits());
    let hostname = match js_string_to_rust(hostname_value) {
        Some(hostname) if !hostname.is_empty() => hostname,
        Some(_) => return promise_rejected_value(invalid_hostname_value_error(hostname_value)),
        None if hostname_js.is_undefined() || hostname_js.is_null() => {
            return promise_rejected_value(invalid_hostname_value_error(hostname_value));
        }
        None => throw_error_value(invalid_hostname_error(hostname_value)),
    };
    let options = match parse_lookup_options(arg(args, 1)) {
        Ok(options) => options,
        Err(error) => throw_error_value(error),
    };
    match lookup_value(&hostname, options) {
        Ok(value) => promise_value(value),
        Err(error) => promise_rejected_value(error),
    }
}

#[no_mangle]
pub extern "C" fn js_dns_promises_lookup_service(args: i64) -> f64 {
    if args_len(args) < 2 {
        throw_error_value(lookup_service_missing_args_error());
    }
    let address_value = arg(args, 0);
    let address = match js_string_to_rust(address_value) {
        Some(address) => address,
        None => throw_error_value(invalid_address_error(address_value)),
    };
    let port = match parse_lookup_service_port(arg(args, 1)) {
        Ok(port) => port,
        Err(error) => throw_error_value(error),
    };
    match lookup_service_result(&address, port) {
        Ok((hostname, service)) => promise_value(lookup_service_object(&hostname, &service)),
        Err(error) => throw_error_value(error),
    }
}

/// Calls the DNS export `method` of `dns` (`promises == false`) or
/// `dns/promises` (`promises == true`) with `args` (a packed `ArrayHeader`
/// pointer, the same shape the static native table passes). Returns `None`
/// when `method` is not a lookup/resolve/reverse export.
///
/// The static native table (`NODE_DNS_ROWS`) resolves these at compile time
/// for ESM and literal `dns.*` calls. A namespace obtained at runtime (the
/// synthetic CommonJS `require`) reaches them through the dynamic native
/// module dispatch instead, which funnels here through [`dispatch_export`].
fn route_export(promises: bool, method: &str, args: i64) -> Option<f64> {
    macro_rules! route {
        ($($name:literal => $cb:ident, $pr:ident;)*) => {
            match (promises, method) {
                $(
                    (false, $name) => Some($cb(args)),
                    (true, $name) => Some($pr(args)),
                )*
                _ => None,
            }
        };
    }
    route! {
        "lookup" => js_dns_lookup, js_dns_promises_lookup;
        "lookupService" => js_dns_lookup_service, js_dns_promises_lookup_service;
        "resolve" => js_dns_resolve, js_dns_promises_resolve;
        "resolve4" => js_dns_resolve4, js_dns_promises_resolve4;
        "resolve6" => js_dns_resolve6, js_dns_promises_resolve6;
        "resolveAny" => js_dns_resolve_any, js_dns_promises_resolve_any;
        "resolveCaa" => js_dns_resolve_caa, js_dns_promises_resolve_caa;
        "resolveCname" => js_dns_resolve_cname, js_dns_promises_resolve_cname;
        "resolveMx" => js_dns_resolve_mx, js_dns_promises_resolve_mx;
        "resolveNaptr" => js_dns_resolve_naptr, js_dns_promises_resolve_naptr;
        "resolveNs" => js_dns_resolve_ns, js_dns_promises_resolve_ns;
        "resolvePtr" => js_dns_resolve_ptr, js_dns_promises_resolve_ptr;
        "resolveSoa" => js_dns_resolve_soa, js_dns_promises_resolve_soa;
        "resolveSrv" => js_dns_resolve_srv, js_dns_promises_resolve_srv;
        "resolveTlsa" => js_dns_resolve_tlsa, js_dns_promises_resolve_tlsa;
        "resolveTxt" => js_dns_resolve_txt, js_dns_promises_resolve_txt;
        "reverse" => js_dns_reverse, js_dns_promises_reverse;
    }
}

type ExportRouter = fn(bool, &str, i64) -> Option<f64>;

/// The router, once a program that names `dns` armed it. The dynamic native
/// module dispatch is linked into every CommonJS program (all buckets are
/// pinned there), so it must not reference the lookup and resolve FFIs
/// directly: that would link the lookup and resolve FFIs, and the host
/// resolver they reach, into programs that never use `dns`. Only
/// `js_dns_install_exports`, which codegen emits for programs that import or
/// require `dns` / `dns/promises`, references them.
static EXPORT_ROUTER: std::sync::atomic::AtomicPtr<()> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

#[no_mangle]
pub extern "C" fn js_dns_install_exports() {
    // `black_box` keeps the store opaque: with a single stored value the
    // optimizer would devirtualize the load in `dispatch_export` into a direct
    // reference and pin the FFIs in every program that links the dispatch.
    let router = std::hint::black_box(route_export as ExportRouter);
    EXPORT_ROUTER.store(router as *mut (), std::sync::atomic::Ordering::Relaxed);
}

/// [`route_export`] when this program armed it, `None` otherwise.
pub(crate) fn dispatch_export(promises: bool, method: &str, args: i64) -> Option<f64> {
    let p = EXPORT_ROUTER.load(std::sync::atomic::Ordering::Relaxed);
    if p.is_null() {
        return None;
    }
    let router = unsafe { std::mem::transmute::<*mut (), ExportRouter>(p) };
    router(promises, method, args)
}
