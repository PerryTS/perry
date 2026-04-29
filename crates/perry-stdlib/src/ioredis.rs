//! Redis client module (ioredis compatible)
//!
//! Native implementation of the 'ioredis' npm package using the Rust redis crate.
//! Provides async Redis operations with lazy connection (like real ioredis).

use perry_runtime::{js_string_from_bytes, JSValue, StringHeader};
use redis::AsyncCommands;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use crate::common::async_bridge::{queue_deferred_resolution, queue_promise_resolution, spawn};
use crate::common::{register_handle, Handle};

/// Default timeout for Redis operations
const DEFAULT_TIMEOUT_SECS: u64 = 10;

/// Redis client handle - stores connection URL and cached connection
pub(crate) struct RedisClient {
    url: String,
}

lazy_static::lazy_static! {
    /// Shared connection pool - connections are cached by URL
    static ref CONNECTIONS: Mutex<HashMap<Handle, redis::aio::MultiplexedConnection>> = Mutex::new(HashMap::new());
    /// URL storage for handles
    static ref URLS: Mutex<HashMap<Handle, String>> = Mutex::new(HashMap::new());
}

/// Helper to extract string from StringHeader pointer
unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    std::str::from_utf8(bytes).ok().map(|s| s.to_string())
}

/// Create a new Redis client (synchronous, connects lazily like real ioredis)
/// new Redis() or new Redis(options)
#[no_mangle]
pub unsafe extern "C" fn js_ioredis_new(_config_ptr: *const std::ffi::c_void) -> Handle {
    // Build connection URL from environment variables (same as TS redis-config.ts)
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

    // Register handle and store URL
    let handle = register_handle(RedisClient { url: url.clone() });
    URLS.lock().unwrap().insert(handle, url);
    handle
}

/// Get or create a connection for the given handle
async fn get_connection(handle: Handle) -> Result<redis::aio::MultiplexedConnection, String> {
    // Check if we already have a connection
    {
        let conns = CONNECTIONS.lock().unwrap();
        if let Some(conn) = conns.get(&handle) {
            return Ok(conn.clone());
        }
    }

    // Get URL for this handle
    let url = {
        let urls = URLS.lock().unwrap();
        urls.get(&handle).cloned()
    };

    let url = url.ok_or_else(|| "Invalid Redis handle".to_string())?;

    // Create new connection with timeout
    let client =
        redis::Client::open(url.as_str()).map_err(|e| format!("Redis client error: {}", e))?;

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

    // Cache the connection
    CONNECTIONS.lock().unwrap().insert(handle, conn.clone());

    Ok(conn)
}

/// SET command
/// redis.set(key, value) -> Promise<"OK">
#[no_mangle]
pub unsafe extern "C" fn js_ioredis_set(
    handle: Handle,
    key_ptr: *const StringHeader,
    value_ptr: *const StringHeader,
) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new();
    let promise_ptr = promise as usize;

    let key = match string_from_header(key_ptr) {
        Some(k) => k,
        None => {
            let err_msg = "Invalid key";
            let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
            queue_promise_resolution(promise_ptr, false, JSValue::string_ptr(err_str).bits());
            return promise;
        }
    };

    let value = match string_from_header(value_ptr) {
        Some(v) => v,
        None => {
            let err_msg = "Invalid value";
            let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
            queue_promise_resolution(promise_ptr, false, JSValue::string_ptr(err_str).bits());
            return promise;
        }
    };

    spawn(async move {
        match get_connection(handle).await {
            Ok(mut conn) => {
                let result: redis::RedisResult<()> = tokio::time::timeout(
                    Duration::from_secs(DEFAULT_TIMEOUT_SECS),
                    conn.set::<_, _, ()>(&key, &value),
                )
                .await
                .map_err(|_| {
                    redis::RedisError::from((redis::ErrorKind::IoError, "Operation timed out"))
                })
                .and_then(|r| r);

                match result {
                    Ok(_) => {
                        queue_deferred_resolution(promise_ptr, true, || {
                            let ok_str = "OK";
                            let result_str =
                                js_string_from_bytes(ok_str.as_ptr(), ok_str.len() as u32);
                            JSValue::string_ptr(result_str).bits()
                        });
                    }
                    Err(e) => {
                        let err_msg = format!("Redis SET error: {}", e);
                        queue_deferred_resolution(promise_ptr, false, move || {
                            let err_str =
                                js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
                            JSValue::string_ptr(err_str).bits()
                        });
                    }
                }
            }
            Err(e) => {
                queue_deferred_resolution(promise_ptr, false, move || {
                    let err_str = js_string_from_bytes(e.as_ptr(), e.len() as u32);
                    JSValue::string_ptr(err_str).bits()
                });
            }
        }
    });

    promise
}

/// GET command
/// redis.get(key) -> Promise<string | null>
#[no_mangle]
pub unsafe extern "C" fn js_ioredis_get(
    handle: Handle,
    key_ptr: *const StringHeader,
) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new();
    let promise_ptr = promise as usize;

    let key = match string_from_header(key_ptr) {
        Some(k) => k,
        None => {
            let err_msg = "Invalid key";
            let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
            queue_promise_resolution(promise_ptr, false, JSValue::string_ptr(err_str).bits());
            return promise;
        }
    };

    spawn(async move {
        match get_connection(handle).await {
            Ok(mut conn) => {
                let result: redis::RedisResult<Option<String>> =
                    tokio::time::timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS), conn.get(&key))
                        .await
                        .map_err(|_| {
                            redis::RedisError::from((
                                redis::ErrorKind::IoError,
                                "Operation timed out",
                            ))
                        })
                        .and_then(|r| r);

                match result {
                    Ok(Some(value)) => {
                        queue_deferred_resolution(promise_ptr, true, move || {
                            let result_str =
                                js_string_from_bytes(value.as_ptr(), value.len() as u32);
                            JSValue::string_ptr(result_str).bits()
                        });
                    }
                    Ok(None) => {
                        queue_promise_resolution(promise_ptr, true, JSValue::null().bits());
                    }
                    Err(e) => {
                        let err_msg = format!("Redis GET error: {}", e);
                        queue_deferred_resolution(promise_ptr, false, move || {
                            let err_str =
                                js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
                            JSValue::string_ptr(err_str).bits()
                        });
                    }
                }
            }
            Err(e) => {
                queue_deferred_resolution(promise_ptr, false, move || {
                    let err_str = js_string_from_bytes(e.as_ptr(), e.len() as u32);
                    JSValue::string_ptr(err_str).bits()
                });
            }
        }
    });

    promise
}

/// DEL command
/// redis.del(key) -> Promise<number>
#[no_mangle]
pub unsafe extern "C" fn js_ioredis_del(
    handle: Handle,
    key_ptr: *const StringHeader,
) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new();
    let promise_ptr = promise as usize;

    let key = match string_from_header(key_ptr) {
        Some(k) => k,
        None => {
            let err_msg = "Invalid key";
            let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
            queue_promise_resolution(promise_ptr, false, JSValue::string_ptr(err_str).bits());
            return promise;
        }
    };

    spawn(async move {
        match get_connection(handle).await {
            Ok(mut conn) => {
                let result: redis::RedisResult<i64> =
                    tokio::time::timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECS), conn.del(&key))
                        .await
                        .map_err(|_| {
                            redis::RedisError::from((
                                redis::ErrorKind::IoError,
                                "Operation timed out",
                            ))
                        })
                        .and_then(|r| r);

                match result {
                    Ok(count) => {
                        queue_promise_resolution(promise_ptr, true, (count as f64).to_bits());
                    }
                    Err(e) => {
                        let err_msg = format!("Redis DEL error: {}", e);
                        queue_deferred_resolution(promise_ptr, false, move || {
                            let err_str =
                                js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
                            JSValue::string_ptr(err_str).bits()
                        });
                    }
                }
            }
            Err(e) => {
                queue_deferred_resolution(promise_ptr, false, move || {
                    let err_str = js_string_from_bytes(e.as_ptr(), e.len() as u32);
                    JSValue::string_ptr(err_str).bits()
                });
            }
        }
    });

    promise
}

/// EXISTS command
/// redis.exists(key) -> Promise<number>
#[no_mangle]
pub unsafe extern "C" fn js_ioredis_exists(
    handle: Handle,
    key_ptr: *const StringHeader,
) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new();
    let promise_ptr = promise as usize;

    let key = match string_from_header(key_ptr) {
        Some(k) => k,
        None => {
            let err_msg = "Invalid key";
            let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
            queue_promise_resolution(promise_ptr, false, JSValue::string_ptr(err_str).bits());
            return promise;
        }
    };

    spawn(async move {
        match get_connection(handle).await {
            Ok(mut conn) => {
                let result: redis::RedisResult<i64> = tokio::time::timeout(
                    Duration::from_secs(DEFAULT_TIMEOUT_SECS),
                    conn.exists(&key),
                )
                .await
                .map_err(|_| {
                    redis::RedisError::from((redis::ErrorKind::IoError, "Operation timed out"))
                })
                .and_then(|r| r);

                match result {
                    Ok(count) => {
                        queue_promise_resolution(promise_ptr, true, (count as f64).to_bits());
                    }
                    Err(e) => {
                        let err_msg = format!("Redis EXISTS error: {}", e);
                        queue_deferred_resolution(promise_ptr, false, move || {
                            let err_str =
                                js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
                            JSValue::string_ptr(err_str).bits()
                        });
                    }
                }
            }
            Err(e) => {
                queue_deferred_resolution(promise_ptr, false, move || {
                    let err_str = js_string_from_bytes(e.as_ptr(), e.len() as u32);
                    JSValue::string_ptr(err_str).bits()
                });
            }
        }
    });

    promise
}

/// INCR command
/// redis.incr(key) -> Promise<number>
#[no_mangle]
pub unsafe extern "C" fn js_ioredis_incr(
    handle: Handle,
    key_ptr: *const StringHeader,
) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new();
    let promise_ptr = promise as usize;

    let key = match string_from_header(key_ptr) {
        Some(k) => k,
        None => {
            let err_msg = "Invalid key";
            let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
            queue_promise_resolution(promise_ptr, false, JSValue::string_ptr(err_str).bits());
            return promise;
        }
    };

    spawn(async move {
        match get_connection(handle).await {
            Ok(mut conn) => {
                let result: redis::RedisResult<i64> = tokio::time::timeout(
                    Duration::from_secs(DEFAULT_TIMEOUT_SECS),
                    conn.incr(&key, 1),
                )
                .await
                .map_err(|_| {
                    redis::RedisError::from((redis::ErrorKind::IoError, "Operation timed out"))
                })
                .and_then(|r| r);

                match result {
                    Ok(val) => {
                        queue_promise_resolution(promise_ptr, true, (val as f64).to_bits());
                    }
                    Err(e) => {
                        let err_msg = format!("Redis INCR error: {}", e);
                        queue_deferred_resolution(promise_ptr, false, move || {
                            let err_str =
                                js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
                            JSValue::string_ptr(err_str).bits()
                        });
                    }
                }
            }
            Err(e) => {
                queue_deferred_resolution(promise_ptr, false, move || {
                    let err_str = js_string_from_bytes(e.as_ptr(), e.len() as u32);
                    JSValue::string_ptr(err_str).bits()
                });
            }
        }
    });

    promise
}

/// DECR command
/// redis.decr(key) -> Promise<number>
#[no_mangle]
pub unsafe extern "C" fn js_ioredis_decr(
    handle: Handle,
    key_ptr: *const StringHeader,
) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new();
    let promise_ptr = promise as usize;

    let key = match string_from_header(key_ptr) {
        Some(k) => k,
        None => {
            let err_msg = "Invalid key";
            let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
            queue_promise_resolution(promise_ptr, false, JSValue::string_ptr(err_str).bits());
            return promise;
        }
    };

    spawn(async move {
        match get_connection(handle).await {
            Ok(mut conn) => {
                let result: redis::RedisResult<i64> = tokio::time::timeout(
                    Duration::from_secs(DEFAULT_TIMEOUT_SECS),
                    conn.decr(&key, 1),
                )
                .await
                .map_err(|_| {
                    redis::RedisError::from((redis::ErrorKind::IoError, "Operation timed out"))
                })
                .and_then(|r| r);

                match result {
                    Ok(val) => {
                        queue_promise_resolution(promise_ptr, true, (val as f64).to_bits());
                    }
                    Err(e) => {
                        let err_msg = format!("Redis DECR error: {}", e);
                        queue_deferred_resolution(promise_ptr, false, move || {
                            let err_str =
                                js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
                            JSValue::string_ptr(err_str).bits()
                        });
                    }
                }
            }
            Err(e) => {
                queue_deferred_resolution(promise_ptr, false, move || {
                    let err_str = js_string_from_bytes(e.as_ptr(), e.len() as u32);
                    JSValue::string_ptr(err_str).bits()
                });
            }
        }
    });

    promise
}

/// EXPIRE command
/// redis.expire(key, seconds) -> Promise<number>
#[no_mangle]
pub unsafe extern "C" fn js_ioredis_expire(
    handle: Handle,
    key_ptr: *const StringHeader,
    seconds: f64,
) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new();
    let promise_ptr = promise as usize;

    let key = match string_from_header(key_ptr) {
        Some(k) => k,
        None => {
            let err_msg = "Invalid key";
            let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
            queue_promise_resolution(promise_ptr, false, JSValue::string_ptr(err_str).bits());
            return promise;
        }
    };

    let secs = seconds as i64;

    spawn(async move {
        match get_connection(handle).await {
            Ok(mut conn) => {
                let result: redis::RedisResult<i64> = tokio::time::timeout(
                    Duration::from_secs(DEFAULT_TIMEOUT_SECS),
                    conn.expire(&key, secs),
                )
                .await
                .map_err(|_| {
                    redis::RedisError::from((redis::ErrorKind::IoError, "Operation timed out"))
                })
                .and_then(|r| r);

                match result {
                    Ok(val) => {
                        queue_promise_resolution(promise_ptr, true, (val as f64).to_bits());
                    }
                    Err(e) => {
                        let err_msg = format!("Redis EXPIRE error: {}", e);
                        queue_deferred_resolution(promise_ptr, false, move || {
                            let err_str =
                                js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
                            JSValue::string_ptr(err_str).bits()
                        });
                    }
                }
            }
            Err(e) => {
                queue_deferred_resolution(promise_ptr, false, move || {
                    let err_str = js_string_from_bytes(e.as_ptr(), e.len() as u32);
                    JSValue::string_ptr(err_str).bits()
                });
            }
        }
    });

    promise
}

/// CONNECT command - explicitly establish connection
/// redis.connect() -> Promise<void>
#[no_mangle]
pub unsafe extern "C" fn js_ioredis_connect(handle: Handle) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new();
    let promise_ptr = promise as usize;

    spawn(async move {
        match get_connection(handle).await {
            Ok(_) => {
                queue_promise_resolution(promise_ptr, true, JSValue::undefined().bits());
            }
            Err(e) => {
                queue_deferred_resolution(promise_ptr, false, move || {
                    let err_str = js_string_from_bytes(e.as_ptr(), e.len() as u32);
                    JSValue::string_ptr(err_str).bits()
                });
            }
        }
    });

    promise
}

/// SETEX command - set key with expiration
/// redis.setex(key, seconds, value) -> Promise<"OK">
#[no_mangle]
pub unsafe extern "C" fn js_ioredis_setex(
    handle: Handle,
    key_ptr: *const StringHeader,
    seconds: f64,
    value_ptr: *const StringHeader,
) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new();
    let promise_ptr = promise as usize;

    let key = match string_from_header(key_ptr) {
        Some(k) => k,
        None => {
            let err_msg = "Invalid key";
            let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
            queue_promise_resolution(promise_ptr, false, JSValue::string_ptr(err_str).bits());
            return promise;
        }
    };

    let value = match string_from_header(value_ptr) {
        Some(v) => v,
        None => {
            let err_msg = "Invalid value";
            let err_str = js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
            queue_promise_resolution(promise_ptr, false, JSValue::string_ptr(err_str).bits());
            return promise;
        }
    };

    let secs = seconds as u64;

    spawn(async move {
        match get_connection(handle).await {
            Ok(mut conn) => {
                let result: redis::RedisResult<()> = tokio::time::timeout(
                    Duration::from_secs(DEFAULT_TIMEOUT_SECS),
                    conn.set_ex::<_, _, ()>(&key, &value, secs),
                )
                .await
                .map_err(|_| {
                    redis::RedisError::from((redis::ErrorKind::IoError, "Operation timed out"))
                })
                .and_then(|r| r);

                match result {
                    Ok(_) => {
                        queue_deferred_resolution(promise_ptr, true, || {
                            let ok_str = "OK";
                            let result_str =
                                js_string_from_bytes(ok_str.as_ptr(), ok_str.len() as u32);
                            JSValue::string_ptr(result_str).bits()
                        });
                    }
                    Err(e) => {
                        let err_msg = format!("Redis SETEX error: {}", e);
                        queue_deferred_resolution(promise_ptr, false, move || {
                            let err_str =
                                js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
                            JSValue::string_ptr(err_str).bits()
                        });
                    }
                }
            }
            Err(e) => {
                queue_deferred_resolution(promise_ptr, false, move || {
                    let err_str = js_string_from_bytes(e.as_ptr(), e.len() as u32);
                    JSValue::string_ptr(err_str).bits()
                });
            }
        }
    });

    promise
}

/// DISCONNECT command - close connection
/// redis.disconnect() -> void (synchronous in ioredis)
#[no_mangle]
pub unsafe extern "C" fn js_ioredis_disconnect(handle: Handle) {
    CONNECTIONS.lock().unwrap().remove(&handle);
}

/// PING command - test connection
/// redis.ping() -> Promise<"PONG">
#[no_mangle]
pub unsafe extern "C" fn js_ioredis_ping(handle: Handle) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new();
    let promise_ptr = promise as usize;

    spawn(async move {
        match get_connection(handle).await {
            Ok(mut conn) => {
                let result: redis::RedisResult<String> = tokio::time::timeout(
                    Duration::from_secs(DEFAULT_TIMEOUT_SECS),
                    redis::cmd("PING").query_async(&mut conn),
                )
                .await
                .map_err(|_| {
                    redis::RedisError::from((redis::ErrorKind::IoError, "Operation timed out"))
                })
                .and_then(|r| r);

                match result {
                    Ok(pong) => {
                        queue_deferred_resolution(promise_ptr, true, move || {
                            let result_str = js_string_from_bytes(pong.as_ptr(), pong.len() as u32);
                            JSValue::string_ptr(result_str).bits()
                        });
                    }
                    Err(e) => {
                        let err_msg = format!("Redis PING error: {}", e);
                        queue_deferred_resolution(promise_ptr, false, move || {
                            let err_str =
                                js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
                            JSValue::string_ptr(err_str).bits()
                        });
                    }
                }
            }
            Err(e) => {
                queue_deferred_resolution(promise_ptr, false, move || {
                    let err_str = js_string_from_bytes(e.as_ptr(), e.len() as u32);
                    JSValue::string_ptr(err_str).bits()
                });
            }
        }
    });

    promise
}

/// HGET command
/// redis.hget(key, field) -> Promise<string | null>
#[no_mangle]
pub unsafe extern "C" fn js_ioredis_hget(
    handle: Handle,
    key_ptr: *const StringHeader,
    field_ptr: *const StringHeader,
) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new();
    let promise_ptr = promise as usize;

    let key = match string_from_header(key_ptr) {
        Some(k) => k,
        None => {
            queue_promise_resolution(promise_ptr, true, JSValue::null().bits());
            return promise;
        }
    };

    let field = match string_from_header(field_ptr) {
        Some(f) => f,
        None => {
            queue_promise_resolution(promise_ptr, true, JSValue::null().bits());
            return promise;
        }
    };

    spawn(async move {
        match get_connection(handle).await {
            Ok(mut conn) => {
                let result: redis::RedisResult<Option<String>> = tokio::time::timeout(
                    Duration::from_secs(DEFAULT_TIMEOUT_SECS),
                    conn.hget(&key, &field),
                )
                .await
                .map_err(|_| {
                    redis::RedisError::from((redis::ErrorKind::IoError, "Operation timed out"))
                })
                .and_then(|r| r);

                match result {
                    Ok(Some(value)) => {
                        queue_deferred_resolution(promise_ptr, true, move || {
                            let result_str =
                                js_string_from_bytes(value.as_ptr(), value.len() as u32);
                            JSValue::string_ptr(result_str).bits()
                        });
                    }
                    Ok(None) => {
                        queue_promise_resolution(promise_ptr, true, JSValue::null().bits());
                    }
                    Err(e) => {
                        let err_msg = format!("Redis HGET error: {}", e);
                        queue_deferred_resolution(promise_ptr, false, move || {
                            let err_str =
                                js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
                            JSValue::string_ptr(err_str).bits()
                        });
                    }
                }
            }
            Err(e) => {
                queue_deferred_resolution(promise_ptr, false, move || {
                    let err_str = js_string_from_bytes(e.as_ptr(), e.len() as u32);
                    JSValue::string_ptr(err_str).bits()
                });
            }
        }
    });

    promise
}

/// HSET command
/// redis.hset(key, field, value) -> Promise<number>
#[no_mangle]
pub unsafe extern "C" fn js_ioredis_hset(
    handle: Handle,
    key_ptr: *const StringHeader,
    field_ptr: *const StringHeader,
    value_ptr: *const StringHeader,
) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new();
    let promise_ptr = promise as usize;

    let key = match string_from_header(key_ptr) {
        Some(k) => k,
        None => {
            queue_promise_resolution(promise_ptr, true, (0f64).to_bits());
            return promise;
        }
    };

    let field = match string_from_header(field_ptr) {
        Some(f) => f,
        None => {
            queue_promise_resolution(promise_ptr, true, (0f64).to_bits());
            return promise;
        }
    };

    let value = match string_from_header(value_ptr) {
        Some(v) => v,
        None => {
            queue_promise_resolution(promise_ptr, true, (0f64).to_bits());
            return promise;
        }
    };

    spawn(async move {
        match get_connection(handle).await {
            Ok(mut conn) => {
                let result: redis::RedisResult<i64> = tokio::time::timeout(
                    Duration::from_secs(DEFAULT_TIMEOUT_SECS),
                    conn.hset(&key, &field, &value),
                )
                .await
                .map_err(|_| {
                    redis::RedisError::from((redis::ErrorKind::IoError, "Operation timed out"))
                })
                .and_then(|r| r);

                match result {
                    Ok(count) => {
                        queue_promise_resolution(promise_ptr, true, (count as f64).to_bits());
                    }
                    Err(e) => {
                        let err_msg = format!("Redis HSET error: {}", e);
                        queue_deferred_resolution(promise_ptr, false, move || {
                            let err_str =
                                js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
                            JSValue::string_ptr(err_str).bits()
                        });
                    }
                }
            }
            Err(e) => {
                queue_deferred_resolution(promise_ptr, false, move || {
                    let err_str = js_string_from_bytes(e.as_ptr(), e.len() as u32);
                    JSValue::string_ptr(err_str).bits()
                });
            }
        }
    });

    promise
}

/// HGETALL command
/// redis.hgetall(key) -> Promise<Record<string, string>>
#[no_mangle]
pub unsafe extern "C" fn js_ioredis_hgetall(
    handle: Handle,
    key_ptr: *const StringHeader,
) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new();
    let promise_ptr = promise as usize;

    let key = match string_from_header(key_ptr) {
        Some(k) => k,
        None => {
            // Return empty object for invalid key
            queue_deferred_resolution(promise_ptr, true, || {
                let obj = perry_runtime::object::js_object_alloc(0, 0);
                JSValue::pointer(obj as *mut u8).bits()
            });
            return promise;
        }
    };

    spawn(async move {
        match get_connection(handle).await {
            Ok(mut conn) => {
                let result: redis::RedisResult<HashMap<String, String>> = tokio::time::timeout(
                    Duration::from_secs(DEFAULT_TIMEOUT_SECS),
                    conn.hgetall(&key),
                )
                .await
                .map_err(|_| {
                    redis::RedisError::from((redis::ErrorKind::IoError, "Operation timed out"))
                })
                .and_then(|r| r);

                match result {
                    Ok(hash) => {
                        // Collect entries so we can move them into the closure
                        let entries: Vec<(String, String)> = hash.into_iter().collect();
                        queue_deferred_resolution(promise_ptr, true, move || {
                            // Create a Perry object with dynamic fields
                            let obj =
                                perry_runtime::object::js_object_alloc(0, entries.len() as u32);
                            for (k, v) in &entries {
                                let key_str = js_string_from_bytes(k.as_ptr(), k.len() as u32);
                                let val_str = js_string_from_bytes(v.as_ptr(), v.len() as u32);
                                let val_bits = JSValue::string_ptr(val_str).bits();
                                perry_runtime::object::js_object_set_field_by_name(
                                    obj,
                                    key_str,
                                    f64::from_bits(val_bits),
                                );
                            }
                            JSValue::pointer(obj as *mut u8).bits()
                        });
                    }
                    Err(e) => {
                        let err_msg = format!("Redis HGETALL error: {}", e);
                        queue_deferred_resolution(promise_ptr, false, move || {
                            let err_str =
                                js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
                            JSValue::string_ptr(err_str).bits()
                        });
                    }
                }
            }
            Err(e) => {
                queue_deferred_resolution(promise_ptr, false, move || {
                    let err_str = js_string_from_bytes(e.as_ptr(), e.len() as u32);
                    JSValue::string_ptr(err_str).bits()
                });
            }
        }
    });

    promise
}

/// HDEL command
/// redis.hdel(key, field) -> Promise<number>
#[no_mangle]
pub unsafe extern "C" fn js_ioredis_hdel(
    handle: Handle,
    key_ptr: *const StringHeader,
    field_ptr: *const StringHeader,
) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new();
    let promise_ptr = promise as usize;

    let key = match string_from_header(key_ptr) {
        Some(k) => k,
        None => {
            queue_promise_resolution(promise_ptr, true, (0f64).to_bits());
            return promise;
        }
    };

    let field = match string_from_header(field_ptr) {
        Some(f) => f,
        None => {
            queue_promise_resolution(promise_ptr, true, (0f64).to_bits());
            return promise;
        }
    };

    spawn(async move {
        match get_connection(handle).await {
            Ok(mut conn) => {
                let result: redis::RedisResult<i64> = tokio::time::timeout(
                    Duration::from_secs(DEFAULT_TIMEOUT_SECS),
                    conn.hdel(&key, &field),
                )
                .await
                .map_err(|_| {
                    redis::RedisError::from((redis::ErrorKind::IoError, "Operation timed out"))
                })
                .and_then(|r| r);

                match result {
                    Ok(count) => {
                        queue_promise_resolution(promise_ptr, true, (count as f64).to_bits());
                    }
                    Err(e) => {
                        let err_msg = format!("Redis HDEL error: {}", e);
                        queue_deferred_resolution(promise_ptr, false, move || {
                            let err_str =
                                js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
                            JSValue::string_ptr(err_str).bits()
                        });
                    }
                }
            }
            Err(e) => {
                queue_deferred_resolution(promise_ptr, false, move || {
                    let err_str = js_string_from_bytes(e.as_ptr(), e.len() as u32);
                    JSValue::string_ptr(err_str).bits()
                });
            }
        }
    });

    promise
}

/// HLEN command
/// redis.hlen(key) -> Promise<number>
#[no_mangle]
pub unsafe extern "C" fn js_ioredis_hlen(
    handle: Handle,
    key_ptr: *const StringHeader,
) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new();
    let promise_ptr = promise as usize;

    let key = match string_from_header(key_ptr) {
        Some(k) => k,
        None => {
            queue_promise_resolution(promise_ptr, true, (0f64).to_bits());
            return promise;
        }
    };

    spawn(async move {
        match get_connection(handle).await {
            Ok(mut conn) => {
                let result: redis::RedisResult<i64> = tokio::time::timeout(
                    Duration::from_secs(DEFAULT_TIMEOUT_SECS),
                    conn.hlen(&key),
                )
                .await
                .map_err(|_| {
                    redis::RedisError::from((redis::ErrorKind::IoError, "Operation timed out"))
                })
                .and_then(|r| r);

                match result {
                    Ok(len) => {
                        queue_promise_resolution(promise_ptr, true, (len as f64).to_bits());
                    }
                    Err(e) => {
                        let err_msg = format!("Redis HLEN error: {}", e);
                        queue_deferred_resolution(promise_ptr, false, move || {
                            let err_str =
                                js_string_from_bytes(err_msg.as_ptr(), err_msg.len() as u32);
                            JSValue::string_ptr(err_str).bits()
                        });
                    }
                }
            }
            Err(e) => {
                queue_deferred_resolution(promise_ptr, false, move || {
                    let err_str = js_string_from_bytes(e.as_ptr(), e.len() as u32);
                    JSValue::string_ptr(err_str).bits()
                });
            }
        }
    });

    promise
}

/// QUIT command - close connection
/// redis.quit() -> Promise<"OK">
#[no_mangle]
pub unsafe extern "C" fn js_ioredis_quit(handle: Handle) -> *mut perry_runtime::Promise {
    let promise = perry_runtime::js_promise_new();
    let promise_ptr = promise as usize;

    // Remove connection from cache
    CONNECTIONS.lock().unwrap().remove(&handle);
    URLS.lock().unwrap().remove(&handle);

    // Return OK immediately
    queue_deferred_resolution(promise_ptr, true, || {
        let ok_str = "OK";
        let result_str = js_string_from_bytes(ok_str.as_ptr(), ok_str.len() as u32);
        JSValue::string_ptr(result_str).bits()
    });

    promise
}
