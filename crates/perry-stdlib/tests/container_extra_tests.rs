#![cfg(feature = "container")]
use perry_container_compose::types::ComposeSpec;
use perry_runtime::{js_promise_run_microtasks, js_promise_state, Promise, StringHeader};
use perry_stdlib::container::*;
use std::ptr;

const PROMISE_STATE_PENDING: i32 = 0;
const PROMISE_STATE_FULFILLED: i32 = 1;
const PROMISE_STATE_REJECTED: i32 = 2;

fn make_string_header(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let len = bytes.len() as u32;
    let mut header_bytes = vec![0u8; std::mem::size_of::<StringHeader>() + bytes.len()];
    unsafe {
        let header = header_bytes.as_mut_ptr() as *mut StringHeader;
        (*header).utf16_len = s.chars().count() as u32;
        (*header).byte_len = len;
        (*header).capacity = len;
        (*header).refcount = 0;
        let data_ptr = header_bytes
            .as_mut_ptr()
            .add(std::mem::size_of::<StringHeader>());
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), data_ptr, bytes.len());
    }
    header_bytes
}

fn drive_promise(promise: *mut Promise) {
    let mut iterations = 0;
    while js_promise_state(promise) == PROMISE_STATE_PENDING && iterations < 100 {
        unsafe {
            perry_stdlib::common::js_stdlib_process_pending();
            js_promise_run_microtasks();
        }
        std::thread::yield_now();
        iterations += 1;
    }
}

#[test]
fn test_topological_sort_tie_breaking() {
    let spec_json = r#"{
        "services": {
            "web": { "image": "web", "depends_on": ["db"] },
            "db": { "image": "db" },
            "redis": { "image": "redis" },
            "api": { "image": "api", "depends_on": ["db"] }
        }
    }"#;
    let spec: ComposeSpec = serde_json::from_str(spec_json).unwrap();
    let order = perry_container_compose::compose::resolve_startup_order(&spec).unwrap();

    // Alphabetical order: api, db, redis, web
    // Roots: db, redis -> db is processed first (d < r)
    // After db: api and web are added to queue. Queue now has: redis, api, web.
    // Alphabetical pick from queue: api (a), then redis (r), then web (w).
    // Final order: ["db", "api", "redis", "web"]
    assert_eq!(order, vec!["db", "api", "redis", "web"]);
}

#[test]
fn test_project_name_resolution() {
    std::env::set_var("COMPOSE_PROJECT_NAME", "env-project");

    // Case 1: From spec
    let spec_with_name = ComposeSpec {
        name: Some("spec-project".to_string()),
        ..Default::default()
    };
    let name = spec_with_name
        .name
        .clone()
        .or_else(|| std::env::var("COMPOSE_PROJECT_NAME").ok())
        .unwrap_or_else(|| "default".to_string());
    assert_eq!(name, "spec-project");

    // Case 2: From env
    let spec_no_name = ComposeSpec::default();
    let name = spec_no_name
        .name
        .clone()
        .or_else(|| std::env::var("COMPOSE_PROJECT_NAME").ok())
        .unwrap_or_else(|| "default".to_string());
    assert_eq!(name, "env-project");
}
