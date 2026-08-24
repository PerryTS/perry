//! Swift Foundation Models bridge for `perry/ios` (#5536).
//!
//! Foundation Models is Swift-only, so the final iOS link compiles the small
//! companion in `swift/PerryFoundationModels.swift`. This Rust side owns the
//! Perry ABI, UTF-8 conversion, Promise lifetime, and owner-agent handoff.

use perry_ffi::copy_string_from_raw as str_from_header;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

type Completion = unsafe extern "C" fn(i64, bool, *const u8, i32);

extern "C" {
    fn perry_swift_foundation_model_availability() -> i32;
    fn perry_swift_foundation_model_session_create(bytes: *const u8, len: i32) -> i64;
    fn perry_swift_foundation_model_session_destroy(session: i64);
    fn perry_swift_foundation_model_respond(
        session: i64,
        bytes: *const u8,
        len: i32,
        context: i64,
        completion: Completion,
    );
    fn js_string_from_bytes(bytes: *const u8, len: u32)
        -> *mut perry_runtime::string::StringHeader;
}

/// Promise address → owner agent. The Promise itself is malloc-space pinned
/// until `js_thread_process_pending` settles the queued completion.
static PENDING_RESPONSES: LazyLock<Mutex<HashMap<i64, perry_runtime::agent::AgentId>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn lock_pending() -> std::sync::MutexGuard<'static, HashMap<i64, perry_runtime::agent::AgentId>> {
    match PENDING_RESPONSES.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn runtime_string(value: &str) -> i64 {
    unsafe { js_string_from_bytes(value.as_ptr(), value.len() as u32) as i64 }
}

#[no_mangle]
pub extern "C" fn perry_ios_foundation_model_availability() -> i64 {
    let value = unsafe {
        match perry_swift_foundation_model_availability() {
            1 => "available",
            2 => "deviceNotEligible",
            3 => "appleIntelligenceNotEnabled",
            4 => "modelNotReady",
            _ => "unsupported",
        }
    };
    runtime_string(value)
}

#[no_mangle]
pub extern "C" fn perry_ios_foundation_model_session_create(instructions_ptr: i64) -> i64 {
    let instructions = if instructions_ptr == 0 {
        String::new()
    } else {
        unsafe { str_from_header(instructions_ptr as *const u8) }.to_string()
    };
    unsafe {
        perry_swift_foundation_model_session_create(
            instructions.as_ptr(),
            instructions.len() as i32,
        )
    }
}

#[no_mangle]
pub extern "C" fn perry_ios_foundation_model_session_destroy(session: f64) {
    if session.is_finite() && session > 0.0 {
        unsafe { perry_swift_foundation_model_session_destroy(session as i64) };
    }
}

unsafe extern "C" fn response_completion(context: i64, success: bool, bytes: *const u8, len: i32) {
    let Some(owner) = lock_pending().remove(&context) else {
        return;
    };
    let value = if bytes.is_null() || len <= 0 {
        String::new()
    } else {
        String::from_utf8_lossy(std::slice::from_raw_parts(bytes, len as usize)).into_owned()
    };
    if success {
        perry_runtime::thread::queue_promise_string_result(owner, context as usize, &value);
    } else {
        perry_runtime::thread::queue_promise_string_rejection(owner, context as usize, &value);
    }
}

#[no_mangle]
pub extern "C" fn perry_ios_foundation_model_respond(session: f64, prompt_ptr: i64) -> i64 {
    let prompt = if prompt_ptr == 0 {
        String::new()
    } else {
        unsafe { str_from_header(prompt_ptr as *const u8) }.to_string()
    };

    // The Swift task can outlive every JS reference to the returned promise.
    // Force malloc-space allocation and pin it until its owner-agent queue
    // drains the result; this is the same protocol used by spawn/waitAsync.
    let promise = perry_runtime::promise::js_promise_new_cross_thread();
    unsafe { perry_runtime::thread::pin_promise(promise) };
    perry_runtime::thread::thread_job_begin();
    let context = promise as i64;
    lock_pending().insert(context, perry_runtime::agent::current_agent());

    unsafe {
        perry_swift_foundation_model_respond(
            if session.is_finite() && session > 0.0 {
                session as i64
            } else {
                0
            },
            prompt.as_ptr(),
            prompt.len() as i32,
            context,
            response_completion,
        );
    }
    context
}
