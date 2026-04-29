//! Axios module
//!
//! Native implementation of the 'axios' npm package using reqwest.
//! Provides HTTP client functionality with a promise-based API.

use crate::common::{get_handle, register_handle, spawn_for_promise, Handle};
use perry_runtime::{
    js_promise_new, js_string_from_bytes, JSValue, ObjectHeader, Promise, StringHeader,
};

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
                        Ok(handle as u64)
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
pub unsafe extern "C" fn js_axios_post(
    url_ptr: *const StringHeader,
    data_ptr: *const StringHeader,
) -> *mut Promise {
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

    let body = string_from_header(data_ptr).unwrap_or_default();

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
                        Ok(handle as u64)
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
pub unsafe extern "C" fn js_axios_put(
    url_ptr: *const StringHeader,
    data_ptr: *const StringHeader,
) -> *mut Promise {
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

    let body = string_from_header(data_ptr).unwrap_or_default();

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
                        Ok(handle as u64)
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
                        Ok(handle as u64)
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
pub unsafe extern "C" fn js_axios_patch(
    url_ptr: *const StringHeader,
    data_ptr: *const StringHeader,
) -> *mut Promise {
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

    let body = string_from_header(data_ptr).unwrap_or_default();

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
                        Ok(handle as u64)
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
