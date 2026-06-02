//! Native bindings for the npm `ioredis` Redis client — uses only
//! perry-ffi. Async via `redis::AsyncCommands` bridged through
//! `spawn_blocking` + `JsPromise` + `tokio::Handle::current().block_on`.
//!
//! Mirrors perry-stdlib's existing surface byte-for-byte: lazy
//! connection (cached `MultiplexedConnection` per handle, established
//! on first command), 10-second default timeout, env-var-driven URL
//! construction (`REDIS_HOST` / `REDIS_PORT` / `REDIS_PASSWORD` /
//! `REDIS_TLS`).

use lazy_static::lazy_static;
use perry_ffi::{
    alloc_string, build_object_shape, js_object_alloc_with_shape, js_object_set_field, read_string,
    register_handle, spawn_blocking, take_handle, Handle, JsPromise, JsString, JsValue, Promise,
    StringHeader,
};
use redis::AsyncCommands;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

const DEFAULT_TIMEOUT_SECS: u64 = 10;

pub(crate) struct RedisClient {
    // #854: connection URL is looked up via the URLS side-map at connect time;
    // this mirrored field is retained for the client record but not read back.
    #[allow(dead_code)]
    url: String,
}

lazy_static! {
    static ref CONNECTIONS: Mutex<HashMap<Handle, redis::aio::MultiplexedConnection>> =
        Mutex::new(HashMap::new());
    static ref URLS: Mutex<HashMap<Handle, String>> = Mutex::new(HashMap::new());
}

unsafe fn read_str(ptr: *const StringHeader) -> Option<String> {
    let h = JsString::from_raw(ptr as *mut StringHeader);
    read_string(h).map(String::from)
}

/// `new Redis()` / `new Redis(options)` — sync, lazy connection.
///
/// Builds the URL from environment variables (matches the
/// `redis-config.ts` convention used in perry-stdlib's TS surface):
///   - `REDIS_HOST` (default `127.0.0.1`)
///   - `REDIS_PORT` (default `6379`)
///   - `REDIS_PASSWORD` (default none)
///   - `REDIS_TLS` (default `true`; set to `false` to opt out)
///
/// # Safety
///
/// `_config_ptr` is currently ignored — perry-stdlib's TS surface
/// passes it but the Rust side hasn't wired up per-instance config.
/// Future work: parse a config object via perry-ffi's
/// `js_object_get_field` + read `host` / `port` / `password` etc.
#[no_mangle]
pub unsafe extern "C" fn js_ioredis_new(_config_ptr: *const std::ffi::c_void) -> Handle {
    let host = std::env::var("REDIS_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = std::env::var("REDIS_PORT").unwrap_or_else(|_| "6379".to_string());
    let password = std::env::var("REDIS_PASSWORD").ok();
    let use_tls = std::env::var("REDIS_TLS")
        .map(|v| v != "false")
        .unwrap_or(true);

    let scheme = if use_tls { "rediss" } else { "redis" };
    let url = if let Some(pw) = password {
        format!("{}://:{}@{}:{}", scheme, pw, host, port)
    } else {
        format!("{}://{}:{}", scheme, host, port)
    };

    let handle = register_handle(RedisClient { url: url.clone() });
    URLS.lock().unwrap().insert(handle, url);
    handle
}

async fn get_connection(handle: Handle) -> Result<redis::aio::MultiplexedConnection, String> {
    {
        let conns = CONNECTIONS.lock().unwrap();
        if let Some(conn) = conns.get(&handle) {
            return Ok(conn.clone());
        }
    }
    let url = URLS
        .lock()
        .unwrap()
        .get(&handle)
        .cloned()
        .ok_or_else(|| "Invalid Redis handle".to_string())?;
    let client = redis::Client::open(url).map_err(|e| format!("Redis client error: {}", e))?;
    let conn = tokio::time::timeout(
        Duration::from_secs(DEFAULT_TIMEOUT_SECS),
        client.get_multiplexed_async_connection(),
    )
    .await
    .map_err(|_| {
        format!(
            "Redis connection timed out after {} seconds",
            DEFAULT_TIMEOUT_SECS
        )
    })?
    .map_err(|e| format!("Redis connection error: {}", e))?;
    CONNECTIONS.lock().unwrap().insert(handle, conn.clone());
    Ok(conn)
}

/// Trait for converting Redis results to perry-ffi `JsValue`.
trait ToJsValue {
    fn to_jsvalue(self) -> JsValue;
}

impl ToJsValue for () {
    fn to_jsvalue(self) -> JsValue {
        JsValue::from_string_ptr(alloc_string("OK").as_raw())
    }
}

impl ToJsValue for i64 {
    fn to_jsvalue(self) -> JsValue {
        JsValue::from_number(self as f64)
    }
}

impl ToJsValue for Option<String> {
    fn to_jsvalue(self) -> JsValue {
        match self {
            Some(s) => JsValue::from_string_ptr(alloc_string(&s).as_raw()),
            None => JsValue::NULL,
        }
    }
}

impl ToJsValue for String {
    fn to_jsvalue(self) -> JsValue {
        JsValue::from_string_ptr(alloc_string(&self).as_raw())
    }
}

impl ToJsValue for bool {
    fn to_jsvalue(self) -> JsValue {
        JsValue::from_bool(self)
    }
}

/// Helper that owns the connection-acquisition + timeout boilerplate.
/// `op_label` becomes the prefix in error messages
/// (`Redis SET error: ...`).
fn dispatch<F, Fut, T>(handle: Handle, op_label: &'static str, op: F) -> *mut Promise
where
    F: FnOnce(redis::aio::MultiplexedConnection) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = redis::RedisResult<T>> + Send + 'static,
    T: ToJsValue + Send + 'static,
{
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    spawn_blocking(move || {
        let outcome = tokio::runtime::Handle::current().block_on(async move {
            let conn = get_connection(handle).await?;
            tokio::time::timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS), op(conn))
                .await
                .map_err(|_| format!("Redis {} timed out", op_label))?
                .map_err(|e| format!("Redis {} error: {}", op_label, e))
        });
        match outcome {
            Ok(v) => promise.resolve(v.to_jsvalue()),
            Err(e) => promise.reject_string(&e),
        }
    });
    raw
}

/// `redis.connect() -> Promise<undefined>` — eagerly establish a
/// connection so the next op doesn't pay the connection cost.
#[no_mangle]
pub extern "C" fn js_ioredis_connect(handle: Handle) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    spawn_blocking(move || {
        match tokio::runtime::Handle::current().block_on(get_connection(handle)) {
            Ok(_) => promise.resolve_undefined(),
            Err(e) => promise.reject_string(&e),
        }
    });
    raw
}

/// `redis.set(key, value) -> Promise<"OK">`.
///
/// # Safety
/// Both pointers must be null or Perry-runtime `StringHeader`s.
#[no_mangle]
pub unsafe extern "C" fn js_ioredis_set(
    handle: Handle,
    key_ptr: *const StringHeader,
    value_ptr: *const StringHeader,
) -> *mut Promise {
    let key = read_str(key_ptr).unwrap_or_default();
    let value = read_str(value_ptr).unwrap_or_default();
    dispatch::<_, _, ()>(handle, "SET", move |mut conn| async move {
        conn.set::<_, _, ()>(&key, &value).await
    })
}

/// `redis.setex(key, seconds, value) -> Promise<"OK">`.
///
/// # Safety
/// Both string pointers must be null or Perry-runtime `StringHeader`s.
#[no_mangle]
pub unsafe extern "C" fn js_ioredis_setex(
    handle: Handle,
    key_ptr: *const StringHeader,
    seconds: f64,
    value_ptr: *const StringHeader,
) -> *mut Promise {
    let key = read_str(key_ptr).unwrap_or_default();
    let value = read_str(value_ptr).unwrap_or_default();
    let ttl = seconds.max(0.0) as u64;
    dispatch::<_, _, ()>(handle, "SETEX", move |mut conn| async move {
        conn.set_ex::<_, _, ()>(&key, &value, ttl).await
    })
}

/// `redis.get(key) -> Promise<string | null>`.
///
/// # Safety
/// `key_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_ioredis_get(
    handle: Handle,
    key_ptr: *const StringHeader,
) -> *mut Promise {
    let key = read_str(key_ptr).unwrap_or_default();
    dispatch::<_, _, Option<String>>(handle, "GET", move |mut conn| async move {
        conn.get(&key).await
    })
}

/// `redis.del(key) -> Promise<number>` (count of keys deleted).
///
/// # Safety
/// `key_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_ioredis_del(
    handle: Handle,
    key_ptr: *const StringHeader,
) -> *mut Promise {
    let key = read_str(key_ptr).unwrap_or_default();
    dispatch::<_, _, i64>(handle, "DEL", move |mut conn| async move {
        conn.del(&key).await
    })
}

/// `redis.exists(key) -> Promise<number>` (1 if exists, 0 otherwise).
///
/// # Safety
/// `key_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_ioredis_exists(
    handle: Handle,
    key_ptr: *const StringHeader,
) -> *mut Promise {
    let key = read_str(key_ptr).unwrap_or_default();
    dispatch::<_, _, i64>(handle, "EXISTS", move |mut conn| async move {
        conn.exists(&key).await
    })
}

/// `redis.incr(key) -> Promise<number>`.
///
/// # Safety
/// `key_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_ioredis_incr(
    handle: Handle,
    key_ptr: *const StringHeader,
) -> *mut Promise {
    let key = read_str(key_ptr).unwrap_or_default();
    dispatch::<_, _, i64>(handle, "INCR", move |mut conn| async move {
        conn.incr(&key, 1).await
    })
}

/// `redis.decr(key) -> Promise<number>`.
///
/// # Safety
/// `key_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_ioredis_decr(
    handle: Handle,
    key_ptr: *const StringHeader,
) -> *mut Promise {
    let key = read_str(key_ptr).unwrap_or_default();
    dispatch::<_, _, i64>(handle, "DECR", move |mut conn| async move {
        conn.decr(&key, 1).await
    })
}

/// `redis.expire(key, seconds) -> Promise<number>` (1 if set, 0 otherwise).
///
/// # Safety
/// `key_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_ioredis_expire(
    handle: Handle,
    key_ptr: *const StringHeader,
    seconds: f64,
) -> *mut Promise {
    let key = read_str(key_ptr).unwrap_or_default();
    let ttl = seconds.max(0.0) as i64;
    dispatch::<_, _, i64>(handle, "EXPIRE", move |mut conn| async move {
        conn.expire(&key, ttl).await
    })
}

/// `redis.ping() -> Promise<"PONG">`.
#[no_mangle]
pub extern "C" fn js_ioredis_ping(handle: Handle) -> *mut Promise {
    dispatch::<_, _, String>(handle, "PING", move |mut conn| async move {
        let cmd: redis::Cmd = redis::cmd("PING").to_owned();
        cmd.query_async(&mut conn).await
    })
}

/// `redis.hget(key, field) -> Promise<string | null>`.
///
/// # Safety
/// Both pointers must be null or Perry-runtime `StringHeader`s.
#[no_mangle]
pub unsafe extern "C" fn js_ioredis_hget(
    handle: Handle,
    key_ptr: *const StringHeader,
    field_ptr: *const StringHeader,
) -> *mut Promise {
    let key = read_str(key_ptr).unwrap_or_default();
    let field = read_str(field_ptr).unwrap_or_default();
    dispatch::<_, _, Option<String>>(handle, "HGET", move |mut conn| async move {
        conn.hget(&key, &field).await
    })
}

/// `redis.hset(key, field, value) -> Promise<number>` (count newly set).
///
/// # Safety
/// All three pointers must be null or Perry-runtime `StringHeader`s.
#[no_mangle]
pub unsafe extern "C" fn js_ioredis_hset(
    handle: Handle,
    key_ptr: *const StringHeader,
    field_ptr: *const StringHeader,
    value_ptr: *const StringHeader,
) -> *mut Promise {
    let key = read_str(key_ptr).unwrap_or_default();
    let field = read_str(field_ptr).unwrap_or_default();
    let value = read_str(value_ptr).unwrap_or_default();
    dispatch::<_, _, i64>(handle, "HSET", move |mut conn| async move {
        conn.hset(&key, &field, &value).await
    })
}

/// `redis.hdel(key, field) -> Promise<number>`.
///
/// # Safety
/// Both pointers must be null or Perry-runtime `StringHeader`s.
#[no_mangle]
pub unsafe extern "C" fn js_ioredis_hdel(
    handle: Handle,
    key_ptr: *const StringHeader,
    field_ptr: *const StringHeader,
) -> *mut Promise {
    let key = read_str(key_ptr).unwrap_or_default();
    let field = read_str(field_ptr).unwrap_or_default();
    dispatch::<_, _, i64>(handle, "HDEL", move |mut conn| async move {
        conn.hdel(&key, &field).await
    })
}

/// `redis.hlen(key) -> Promise<number>`.
///
/// # Safety
/// `key_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_ioredis_hlen(
    handle: Handle,
    key_ptr: *const StringHeader,
) -> *mut Promise {
    let key = read_str(key_ptr).unwrap_or_default();
    dispatch::<_, _, i64>(handle, "HLEN", move |mut conn| async move {
        conn.hlen(&key).await
    })
}

/// `redis.hgetall(key) -> Promise<Record<string, string>>`. The
/// resolved object's shape is built dynamically from the keys
/// returned by Redis — same as perry-stdlib's existing copy.
///
/// # Safety
/// `key_ptr` must be null or a Perry-runtime `StringHeader`.
#[no_mangle]
pub unsafe extern "C" fn js_ioredis_hgetall(
    handle: Handle,
    key_ptr: *const StringHeader,
) -> *mut Promise {
    let key = read_str(key_ptr).unwrap_or_default();
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    spawn_blocking(move || {
        let outcome: Result<HashMap<String, String>, String> = tokio::runtime::Handle::current()
            .block_on(async move {
                let mut conn = get_connection(handle).await?;
                tokio::time::timeout(
                    Duration::from_secs(DEFAULT_TIMEOUT_SECS),
                    conn.hgetall::<_, HashMap<String, String>>(&key),
                )
                .await
                .map_err(|_| "Redis HGETALL timed out".to_string())?
                .map_err(|e| format!("Redis HGETALL error: {}", e))
            });
        match outcome {
            Ok(hash) => {
                let entries: Vec<(String, String)> = hash.into_iter().collect();
                let keys: Vec<&str> = entries.iter().map(|(k, _)| k.as_str()).collect();
                let (packed, shape_id) = build_object_shape(&keys);
                let obj = unsafe {
                    js_object_alloc_with_shape(
                        shape_id,
                        entries.len() as u32,
                        packed.as_ptr(),
                        packed.len() as u32,
                    )
                };
                for (i, (_, v)) in entries.iter().enumerate() {
                    let val_str = alloc_string(v);
                    unsafe {
                        js_object_set_field(
                            obj,
                            i as u32,
                            JsValue::from_string_ptr(val_str.as_raw()),
                        );
                    }
                }
                promise.resolve(JsValue::from_object_ptr(obj));
            }
            Err(e) => promise.reject_string(&e),
        }
    });
    raw
}

/// `redis.disconnect()` — drop the cached connection synchronously.
#[no_mangle]
pub extern "C" fn js_ioredis_disconnect(handle: Handle) {
    let mut conns = CONNECTIONS.lock().unwrap();
    conns.remove(&handle);
}

/// `redis.quit() -> Promise<"OK">` — graceful shutdown via QUIT
/// command, then drop the cached connection.
#[no_mangle]
pub extern "C" fn js_ioredis_quit(handle: Handle) -> *mut Promise {
    let promise = JsPromise::new();
    let raw = promise.as_raw();
    spawn_blocking(move || {
        let outcome: Result<(), String> = tokio::runtime::Handle::current().block_on(async move {
            let conn_opt = CONNECTIONS.lock().unwrap().remove(&handle);
            if let Some(mut conn) = conn_opt {
                let _: redis::RedisResult<()> =
                    redis::cmd("QUIT").query_async::<()>(&mut conn).await;
            }
            URLS.lock().unwrap().remove(&handle);
            // Take the handle out of the registry so the wrapper struct drops.
            take_handle::<RedisClient>(handle);
            Ok(())
        });
        match outcome {
            Ok(()) => promise.resolve(JsValue::from_string_ptr(alloc_string("OK").as_raw())),
            Err(e) => promise.reject_string(&e),
        }
    });
    raw
}

// `run_op` exists to satisfy the dead-code lint on the helper trait
// implementations that aren't yet hit by every dispatch caller.
#[allow(dead_code)]
fn _ensure_to_jsvalue_used() -> JsValue {
    let _ = ().to_jsvalue();
    let _ = 0i64.to_jsvalue();
    let _ = (None::<String>).to_jsvalue();
    String::new().to_jsvalue()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_returns_handle() {
        let h = unsafe { js_ioredis_new(std::ptr::null()) };
        assert!(h > 0);
        // URL was stored
        assert!(URLS.lock().unwrap().contains_key(&h));
    }

    #[test]
    fn url_construction_with_password() {
        std::env::set_var("REDIS_HOST", "redis.example.com");
        std::env::set_var("REDIS_PORT", "6380");
        std::env::set_var("REDIS_PASSWORD", "secret");
        std::env::set_var("REDIS_TLS", "true");
        let h = unsafe { js_ioredis_new(std::ptr::null()) };
        let url = URLS.lock().unwrap().get(&h).cloned().unwrap();
        assert!(url.starts_with("rediss://:secret@redis.example.com:6380"));
        std::env::remove_var("REDIS_HOST");
        std::env::remove_var("REDIS_PORT");
        std::env::remove_var("REDIS_PASSWORD");
        std::env::remove_var("REDIS_TLS");
    }

    #[test]
    fn disconnect_clears_cached_conn() {
        let h = unsafe { js_ioredis_new(std::ptr::null()) };
        // No real connection established yet; disconnect is a no-op
        // but should not panic.
        js_ioredis_disconnect(h);
        assert!(!CONNECTIONS.lock().unwrap().contains_key(&h));
    }
}
