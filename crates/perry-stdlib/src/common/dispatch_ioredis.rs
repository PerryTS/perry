//! ioredis Redis-client handle method dispatch.
//!
//! Extracted from `dispatch.rs` to keep that file under the 2000-line
//! limit. `dispatch_property` in the parent module routes `connect`,
//! `get`, `set`, … calls on a `RedisClient` handle here.

/// Dispatch method calls on ioredis Redis client handles
#[cfg(feature = "database-redis")]
pub(super) unsafe fn dispatch_ioredis(handle: i64, method: &str, args: &[f64]) -> f64 {
    // Helper: extract raw StringHeader pointer from NaN-boxed f64
    fn get_string_ptr(val: f64) -> *const perry_runtime::StringHeader {
        let bits = val.to_bits();
        // Strip STRING_TAG (0x7FFF) to get raw pointer
        (bits & 0x0000_FFFF_FFFF_FFFF) as *const perry_runtime::StringHeader
    }

    // Helper: NaN-box a Promise pointer with POINTER_TAG for return
    fn nanbox_promise(promise: *mut perry_runtime::Promise) -> f64 {
        let bits = (promise as u64) | 0x7FFD_0000_0000_0000;
        f64::from_bits(bits)
    }

    match method {
        "connect" => {
            let promise = crate::ioredis::js_ioredis_connect(handle);
            nanbox_promise(promise)
        }
        "get" if !args.is_empty() => {
            let key_ptr = get_string_ptr(args[0]);
            let promise = crate::ioredis::js_ioredis_get(handle, key_ptr);
            nanbox_promise(promise)
        }
        "set" if args.len() >= 2 => {
            let key_ptr = get_string_ptr(args[0]);
            let value_ptr = get_string_ptr(args[1]);
            let promise = crate::ioredis::js_ioredis_set(handle, key_ptr, value_ptr);
            nanbox_promise(promise)
        }
        "setex" if args.len() >= 3 => {
            let key_ptr = get_string_ptr(args[0]);
            let seconds = args[1];
            let value_ptr = get_string_ptr(args[2]);
            let promise = crate::ioredis::js_ioredis_setex(handle, key_ptr, seconds, value_ptr);
            nanbox_promise(promise)
        }
        "del" if !args.is_empty() => {
            let key_ptr = get_string_ptr(args[0]);
            let promise = crate::ioredis::js_ioredis_del(handle, key_ptr);
            nanbox_promise(promise)
        }
        "exists" if !args.is_empty() => {
            let key_ptr = get_string_ptr(args[0]);
            let promise = crate::ioredis::js_ioredis_exists(handle, key_ptr);
            nanbox_promise(promise)
        }
        "incr" if !args.is_empty() => {
            let key_ptr = get_string_ptr(args[0]);
            let promise = crate::ioredis::js_ioredis_incr(handle, key_ptr);
            nanbox_promise(promise)
        }
        "decr" if !args.is_empty() => {
            let key_ptr = get_string_ptr(args[0]);
            let promise = crate::ioredis::js_ioredis_decr(handle, key_ptr);
            nanbox_promise(promise)
        }
        "expire" if args.len() >= 2 => {
            let key_ptr = get_string_ptr(args[0]);
            let seconds = args[1];
            let promise = crate::ioredis::js_ioredis_expire(handle, key_ptr, seconds);
            nanbox_promise(promise)
        }
        "ping" => {
            let promise = crate::ioredis::js_ioredis_ping(handle);
            nanbox_promise(promise)
        }
        "quit" => {
            let promise = crate::ioredis::js_ioredis_quit(handle);
            nanbox_promise(promise)
        }
        "disconnect" => {
            crate::ioredis::js_ioredis_disconnect(handle);
            f64::from_bits(0x7FFC_0000_0000_0001) // undefined
        }
        _ => {
            f64::from_bits(0x7FFC_0000_0000_0001) // undefined
        }
    }
}
