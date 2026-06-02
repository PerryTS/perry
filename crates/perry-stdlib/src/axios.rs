//! Axios module
//!
//! Native implementation of the 'axios' npm package using reqwest.
//! Provides HTTP client functionality with a promise-based API.

use crate::common::{get_handle, register_handle, spawn_for_promise, Handle};
use perry_runtime::{js_promise_new, js_string_from_bytes, Promise, StringHeader};

/// Helper to extract string from StringHeader pointer
unsafe fn string_from_header(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() || (ptr as usize) < 0x1000 {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data_ptr = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data_ptr, len);
    Some(String::from_utf8_lossy(bytes).to_string())
}

/// #598: read the body argument as a JSON string. Strings pass
/// through as-is; everything else is JSON.stringify'd via the
/// runtime's `js_json_stringify`. See perry-ext-axios's parallel
/// helper for the full rationale.
unsafe fn body_string_from_value(value_bits: f64) -> String {
    const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
    const SHORT_STRING_TAG: u64 = 0x7FFB_0000_0000_0000;
    const TAG_MASK: u64 = 0xFFFF_0000_0000_0000;
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let bits = value_bits.to_bits();
    if bits == TAG_UNDEFINED || bits == TAG_NULL {
        return String::new();
    }
    let tag = bits & TAG_MASK;
    if tag == STRING_TAG || tag == SHORT_STRING_TAG {
        let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader;
        return string_from_header(ptr).unwrap_or_default();
    }
    // Object / array / number / etc. — JSON.stringify (type_hint=0
    // = auto-detect from NaN-box tag).
    extern "C" {
        fn js_json_stringify(value: f64, type_hint: u32) -> *mut StringHeader;
    }
    let str_ptr = js_json_stringify(value_bits, 0);
    string_from_header(str_ptr).unwrap_or_default()
}

/// Response handle wrapper
pub struct AxiosResponseHandle {
    pub status: u16,
    pub status_text: String,
    pub data: String,
    pub headers: Vec<(String, String)>,
}

/// axios.get(url) -> Promise<AxiosResponse>
#[no_mangle]
pub unsafe extern "C" fn js_axios_get(url_ptr: *const StringHeader) -> *mut Promise {
    let promise = js_promise_new();

    let url = match string_from_header(url_ptr) {
        Some(u) => u,
        None => {
            spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, _>("Invalid URL".to_string())
            });
            return promise;
        }
    };
    spawn_for_promise(promise as *mut u8, async move {
        let client = reqwest::Client::new();
        match client.get(&url).send().await {
            Ok(response) => {
                let status = response.status().as_u16();
                let status_text = response
                    .status()
                    .canonical_reason()
                    .unwrap_or("")
                    .to_string();
                let headers: Vec<(String, String)> = response
                    .headers()
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
                    .collect();

                match response.text().await {
                    Ok(data) => {
                        let handle = register_handle(AxiosResponseHandle {
                            status,
                            status_text,
                            data,
                            headers,
                        });
                        // Issue #340: NaN-box the handle as POINTER_TAG
                        // (0x7FFD) so the awaiter sees a proper handle
                        // value, not a subnormal float that decays to
                        // undefined on `r.status` / `r.data` accesses.
                        Ok((handle as u64) | 0x7FFD_0000_0000_0000)
                    }
                    Err(e) => Err(format!("Failed to read response body: {}", e)),
                }
            }
            Err(e) => Err(format!("Request failed: {}", e)),
        }
    });

    promise
}

/// axios.post(url, data) -> Promise<AxiosResponse>
#[no_mangle]
pub unsafe extern "C" fn js_axios_post(url_ptr: *const StringHeader, data: f64) -> *mut Promise {
    let promise = js_promise_new();

    let url = match string_from_header(url_ptr) {
        Some(u) => u,
        None => {
            spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, _>("Invalid URL".to_string())
            });
            return promise;
        }
    };

    // #598: stringify on Perry's main thread BEFORE crossing the
    // tokio boundary. `js_json_stringify` reads from perry-runtime's
    // thread-local arena; calling it from inside `spawn_for_promise`
    // would access the wrong arena.
    let body = body_string_from_value(data);

    spawn_for_promise(promise as *mut u8, async move {
        let client = reqwest::Client::new();
        match client
            .post(&url)
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .await
        {
            Ok(response) => {
                let status = response.status().as_u16();
                let status_text = response
                    .status()
                    .canonical_reason()
                    .unwrap_or("")
                    .to_string();
                let headers: Vec<(String, String)> = response
                    .headers()
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
                    .collect();

                match response.text().await {
                    Ok(data) => {
                        let handle = register_handle(AxiosResponseHandle {
                            status,
                            status_text,
                            data,
                            headers,
                        });
                        // Issue #340: NaN-box the handle as POINTER_TAG
                        // (0x7FFD) so the awaiter sees a proper handle
                        // value, not a subnormal float that decays to
                        // undefined on `r.status` / `r.data` accesses.
                        Ok((handle as u64) | 0x7FFD_0000_0000_0000)
                    }
                    Err(e) => Err(format!("Failed to read response body: {}", e)),
                }
            }
            Err(e) => Err(format!("Request failed: {}", e)),
        }
    });

    promise
}

/// axios.put(url, data) -> Promise<AxiosResponse>
#[no_mangle]
pub unsafe extern "C" fn js_axios_put(url_ptr: *const StringHeader, data: f64) -> *mut Promise {
    let promise = js_promise_new();

    let url = match string_from_header(url_ptr) {
        Some(u) => u,
        None => {
            spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, _>("Invalid URL".to_string())
            });
            return promise;
        }
    };

    // #598: stringify on the main thread (see js_axios_post).
    let body = body_string_from_value(data);

    spawn_for_promise(promise as *mut u8, async move {
        let client = reqwest::Client::new();
        match client
            .put(&url)
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .await
        {
            Ok(response) => {
                let status = response.status().as_u16();
                let status_text = response
                    .status()
                    .canonical_reason()
                    .unwrap_or("")
                    .to_string();
                let headers: Vec<(String, String)> = response
                    .headers()
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
                    .collect();

                match response.text().await {
                    Ok(data) => {
                        let handle = register_handle(AxiosResponseHandle {
                            status,
                            status_text,
                            data,
                            headers,
                        });
                        // Issue #340: NaN-box the handle as POINTER_TAG
                        // (0x7FFD) so the awaiter sees a proper handle
                        // value, not a subnormal float that decays to
                        // undefined on `r.status` / `r.data` accesses.
                        Ok((handle as u64) | 0x7FFD_0000_0000_0000)
                    }
                    Err(e) => Err(format!("Failed to read response body: {}", e)),
                }
            }
            Err(e) => Err(format!("Request failed: {}", e)),
        }
    });

    promise
}

/// axios.delete(url) -> Promise<AxiosResponse>
#[no_mangle]
pub unsafe extern "C" fn js_axios_delete(url_ptr: *const StringHeader) -> *mut Promise {
    let promise = js_promise_new();

    let url = match string_from_header(url_ptr) {
        Some(u) => u,
        None => {
            spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, _>("Invalid URL".to_string())
            });
            return promise;
        }
    };

    spawn_for_promise(promise as *mut u8, async move {
        let client = reqwest::Client::new();
        match client.delete(&url).send().await {
            Ok(response) => {
                let status = response.status().as_u16();
                let status_text = response
                    .status()
                    .canonical_reason()
                    .unwrap_or("")
                    .to_string();
                let headers: Vec<(String, String)> = response
                    .headers()
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
                    .collect();

                match response.text().await {
                    Ok(data) => {
                        let handle = register_handle(AxiosResponseHandle {
                            status,
                            status_text,
                            data,
                            headers,
                        });
                        // Issue #340: NaN-box the handle as POINTER_TAG
                        // (0x7FFD) so the awaiter sees a proper handle
                        // value, not a subnormal float that decays to
                        // undefined on `r.status` / `r.data` accesses.
                        Ok((handle as u64) | 0x7FFD_0000_0000_0000)
                    }
                    Err(e) => Err(format!("Failed to read response body: {}", e)),
                }
            }
            Err(e) => Err(format!("Request failed: {}", e)),
        }
    });

    promise
}

/// axios.patch(url, data) -> Promise<AxiosResponse>
#[no_mangle]
pub unsafe extern "C" fn js_axios_patch(url_ptr: *const StringHeader, data: f64) -> *mut Promise {
    let promise = js_promise_new();

    let url = match string_from_header(url_ptr) {
        Some(u) => u,
        None => {
            spawn_for_promise(promise as *mut u8, async move {
                Err::<u64, _>("Invalid URL".to_string())
            });
            return promise;
        }
    };

    // #598: stringify on the main thread (see js_axios_post).
    let body = body_string_from_value(data);

    spawn_for_promise(promise as *mut u8, async move {
        let client = reqwest::Client::new();
        match client
            .patch(&url)
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .await
        {
            Ok(response) => {
                let status = response.status().as_u16();
                let status_text = response
                    .status()
                    .canonical_reason()
                    .unwrap_or("")
                    .to_string();
                let headers: Vec<(String, String)> = response
                    .headers()
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
                    .collect();

                match response.text().await {
                    Ok(data) => {
                        let handle = register_handle(AxiosResponseHandle {
                            status,
                            status_text,
                            data,
                            headers,
                        });
                        // Issue #340: NaN-box the handle as POINTER_TAG
                        // (0x7FFD) so the awaiter sees a proper handle
                        // value, not a subnormal float that decays to
                        // undefined on `r.status` / `r.data` accesses.
                        Ok((handle as u64) | 0x7FFD_0000_0000_0000)
                    }
                    Err(e) => Err(format!("Failed to read response body: {}", e)),
                }
            }
            Err(e) => Err(format!("Request failed: {}", e)),
        }
    });

    promise
}

/// response.status -> number
#[no_mangle]
pub unsafe extern "C" fn js_axios_response_status(handle: Handle) -> f64 {
    if let Some(response) = get_handle::<AxiosResponseHandle>(handle) {
        response.status as f64
    } else {
        0.0
    }
}

/// response.statusText -> string
#[no_mangle]
pub unsafe extern "C" fn js_axios_response_status_text(handle: Handle) -> *mut StringHeader {
    if let Some(response) = get_handle::<AxiosResponseHandle>(handle) {
        js_string_from_bytes(
            response.status_text.as_ptr(),
            response.status_text.len() as u32,
        )
    } else {
        std::ptr::null_mut()
    }
}

/// response.data -> string
#[no_mangle]
pub unsafe extern "C" fn js_axios_response_data(handle: Handle) -> *mut StringHeader {
    if let Some(response) = get_handle::<AxiosResponseHandle>(handle) {
        js_string_from_bytes(response.data.as_ptr(), response.data.len() as u32)
    } else {
        std::ptr::null_mut()
    }
}
