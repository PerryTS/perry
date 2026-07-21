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
    queue_callback(callback_handle.get_nanbox_f64(), &callback_args);
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
    queue_callback(callback_value, &callback_args);
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
pub extern "C" fn js_dns_resolver_new(_args: i64) -> f64 {
    boxed_pointer(resolver_object(stored_servers()) as *const u8)
}

#[no_mangle]
pub extern "C" fn js_dns_promises_resolver_new(_args: i64) -> f64 {
    boxed_pointer(resolver_object(stored_promise_servers()) as *const u8)
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
