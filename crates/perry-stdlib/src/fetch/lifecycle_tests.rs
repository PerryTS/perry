//! Regression tests for `lifecycle`: collections that run while a registry
//! reader is mid-allocation, object-URL roots, young ids, and pacing.

use super::tests::collect;
use super::*;
use std::sync::mpsc;
use std::time::Duration;

/// Larger than `LARGE_OBJECT_THRESHOLD_BYTES` (16 KiB), so the string goes
/// through `gc_malloc`, whose `gc_check_trigger` runs a pending full trace
/// synchronously at the allocation.
const BIG: usize = 64 * 1024;

fn big(fill: char) -> String {
    std::iter::repeat_n(fill, BIG).collect()
}

fn full_traces() -> usize {
    FULL_TRACES.with(Cell::get)
}

/// Keep an id where the conservative stack scan cannot see it: a plain id in
/// a Rust local is a fetch-band word, and an alloc-point collection scans the
/// native stack conservatively, which would keep the "garbage" alive.
struct Hidden(usize);
impl Hidden {
    const MASK: usize = 0x5A5A_0000_0000_0000;
    fn new(id: usize) -> Self {
        Self(id ^ Self::MASK)
    }
    fn id(&self) -> usize {
        self.0 ^ Self::MASK
    }
}

/// An unreachable id whose only native trace is the masked copy returned.
#[inline(never)]
fn garbage(alloc: fn() -> usize) -> Hidden {
    Hidden::new(std::hint::black_box(alloc()))
}

/// Run `case` on a fresh mutator and fail (instead of hanging the suite) if it
/// does not finish: the defect under test is a self-deadlock on a registry
/// mutex, taken by a collection that an allocation under that mutex started.
fn run_without_deadlock(name: &'static str, case: fn()) {
    let (done, finished) = mpsc::channel();
    std::thread::spawn(move || {
        perry_runtime::gc::gc_init();
        case();
        let _ = done.send(());
    });
    match finished.recv_timeout(Duration::from_secs(120)) {
        Ok(()) => {}
        Err(mpsc::RecvTimeoutError::Timeout) => panic!(
            "{name}: deadlocked — a registry guard was held across an allocation \
             that ran a full trace (lock order: see the `lifecycle` module doc)"
        ),
        Err(mpsc::RecvTimeoutError::Disconnected) => panic!("{name}: the case panicked"),
    }
}

/// Arm a full trace for the next GC allocation and return the trace count, so
/// the caller can assert the collection really ran inside the reader.
fn arm_full_trace() -> usize {
    // One collection first: the collector is armed and every id so far is
    // past its young epoch.
    perry_runtime::gc::js_gc_collect();
    request_full_trace();
    full_traces()
}

fn new_response() -> usize {
    handle_id(unsafe { js_response_new(std::ptr::null(), 200.0, std::ptr::null(), 0.0) })
}

fn response_reader_case(read: fn(usize)) {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let id = new_response();
    let _root = scope.root_nanbox_f64(handle_to_f64(id));
    {
        let mut responses = FETCH_RESPONSES.lock().unwrap();
        let response = responses.get_mut(&id).unwrap();
        response.url = big('u');
        response.status_text = big('s');
        response.type_name = big('t');
    }
    let before = arm_full_trace();
    read(id);
    assert!(
        full_traces() > before,
        "no full trace ran inside the reader, so this case tests nothing"
    );
}

#[test]
fn response_string_readers_survive_a_full_trace_in_their_allocation() {
    let _band = crate::fetch::handle_band_test_lock();
    run_without_deadlock("js_fetch_response_url", || {
        response_reader_case(|id| {
            js_fetch_response_url(handle_to_f64(id));
        })
    });
    run_without_deadlock("js_fetch_response_status_text", || {
        response_reader_case(|id| {
            js_fetch_response_status_text(handle_to_f64(id));
        })
    });
    run_without_deadlock("dispatch_response_property(url)", || {
        response_reader_case(|id| {
            dispatch::dispatch_response_property(id, "url").unwrap();
        })
    });
    run_without_deadlock("dispatch_response_property(statusText)", || {
        response_reader_case(|id| {
            dispatch::dispatch_response_property(id, "statusText").unwrap();
        })
    });
    run_without_deadlock("dispatch_response_property(type)", || {
        response_reader_case(|id| {
            dispatch::dispatch_response_property(id, "type").unwrap();
        })
    });
}

/// `release()` locks HEADERS_REGISTRY and BLOB_REGISTRY only when some id died,
/// so these cases leave one old, unreachable id of the right kind behind.
#[test]
fn readers_survive_a_full_trace_that_releases_ids_of_their_own_kind() {
    let _band = crate::fetch::handle_band_test_lock();
    run_without_deadlock("js_headers_setheaders_entries_json", || {
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let headers = js_headers_new();
        let _root = scope.root_nanbox_f64(headers);
        HEADERS_REGISTRY
            .lock()
            .unwrap()
            .get_mut(&handle_id(headers))
            .unwrap()
            .set("x-big", &big('h'));
        let garbage = garbage(|| handle_id(js_headers_new()));
        let before = arm_full_trace();
        js_headers_setheaders_entries_json(headers);
        assert!(
            full_traces() > before,
            "no full trace ran inside the reader"
        );
        assert!(!HEADERS_REGISTRY.lock().unwrap().contains_key(&garbage.id()));
    });
    run_without_deadlock("js_headers_fetch_object_json", || {
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let headers = js_headers_new();
        let _root = scope.root_nanbox_f64(headers);
        HEADERS_REGISTRY
            .lock()
            .unwrap()
            .get_mut(&handle_id(headers))
            .unwrap()
            .set("x-big", &big('h'));
        let garbage = garbage(|| handle_id(js_headers_new()));
        let before = arm_full_trace();
        js_headers_fetch_object_json(headers);
        assert!(
            full_traces() > before,
            "no full trace ran inside the reader"
        );
        assert!(!HEADERS_REGISTRY.lock().unwrap().contains_key(&garbage.id()));
    });
    run_without_deadlock("dispatch_blob_property(type)", || {
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let blob = alloc_blob(BlobData::blob(vec![1], big('b')));
        let _root = scope.root_nanbox_f64(handle_to_f64(blob));
        let garbage = garbage(|| alloc_blob(BlobData::blob(vec![2], String::new())));
        let before = arm_full_trace();
        dispatch::dispatch_blob_property(blob, "type").unwrap();
        assert!(
            full_traces() > before,
            "no full trace ran inside the reader"
        );
        assert!(!BLOB_REGISTRY.lock().unwrap().contains_key(&garbage.id()));
    });
}

/// Node keeps a Blob alive while an object URL names it. Before object URLs
/// were roots, the blob's id was recycled and `resolveObjectURL` handed back
/// whichever handle reused the number.
#[test]
fn object_url_keeps_its_blob_until_revoked() {
    let _band = crate::fetch::handle_band_test_lock();
    std::thread::spawn(|| unsafe {
        perry_runtime::gc::gc_init();
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let blob = alloc_blob(BlobData::blob(b"x".to_vec(), "text/plain".into()));
        let url = crate::fetch_blob::js_url_create_object_url(handle_to_f64(blob));
        let url = scope.root_nanbox_u64(JSValue::string_ptr(url).bits());
        // The blob handle itself is now unreferenced.
        collect();
        // Reuse any recycled ids.
        let reused: Vec<f64> = (0..64).map(|_| js_headers_new()).collect();
        let resolved = crate::fetch_blob::js_buffer_resolve_object_url(url.get_nanbox_f64());
        assert_eq!(resolved.to_bits(), handle_to_f64(blob).to_bits());
        assert!(BLOB_REGISTRY.lock().unwrap().contains_key(&blob));
        assert!(!reused.iter().any(|h| handle_id(*h) == blob));

        crate::fetch_blob::js_url_revoke_object_url(url.get_nanbox_f64());
        collect();
        assert!(!BLOB_REGISTRY.lock().unwrap().contains_key(&blob));
        assert_eq!(
            crate::fetch_blob::js_buffer_resolve_object_url(url.get_nanbox_f64()).to_bits(),
            TAG_UNDEFINED
        );
    })
    .join()
    .unwrap();
}

/// A native frame can hold a fresh id unpublished across an allocation that
/// runs a full trace. Nothing roots a Rust local, so the id — and the heap
/// values its record owns — must survive the first full trace after birth.
#[test]
fn a_young_id_and_its_edges_survive_the_first_full_trace() {
    let _band = crate::fetch::handle_band_test_lock();
    std::thread::spawn(|| {
        perry_runtime::gc::gc_init();
        let id = handle_id(js_headers_new());
        let method = headers_bound_method_value(id, "get");
        let before = full_traces();
        perry_runtime::gc::js_gc_collect();
        assert!(full_traces() > before, "gc() must run a full trace");
        assert!(HEADERS_REGISTRY.lock().unwrap().contains_key(&id));
        // The cached closure was marked through the young root: churn the
        // heap, then check it still names this handle.
        for _ in 0..4096 {
            perry_runtime::js_array_alloc(8);
        }
        assert_eq!(
            headers_bound_method_value(id, "get").to_bits(),
            method.to_bits()
        );
        let closure = perry_runtime::value::js_nanbox_get_pointer(method)
            as *const perry_runtime::closure::ClosureHeader;
        assert_eq!(
            handle_id(perry_runtime::closure::js_closure_get_capture_f64(
                closure, 0
            )),
            id
        );
        // Once old, an unreachable id is reclaimed by the next full trace.
        perry_runtime::gc::js_gc_collect();
        assert!(!HEADERS_REGISTRY.lock().unwrap().contains_key(&id));
    })
    .join()
    .unwrap();
}

/// Pacing follows live growth: after a trace the next request waits for the
/// owned set to double, instead of firing every fixed number of allocations.
#[test]
fn full_trace_requests_scale_with_the_surviving_handle_count() {
    let _band = crate::fetch::handle_band_test_lock();
    std::thread::spawn(|| {
        perry_runtime::gc::gc_init();
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let keep = scope.root_raw_mut_ptr(perry_runtime::js_array_alloc(0));
        let live = 3 * MIN_TRIGGER;
        for _ in 0..live {
            let array = perry_runtime::js_array_push_f64(
                keep.get_raw_mut_ptr::<perry_runtime::ArrayHeader>(),
                js_headers_new(),
            );
            keep.set_raw_mut_ptr(array);
        }
        collect();
        let owned = OWNED.with(|owned| owned.borrow().len());
        assert!(owned >= live, "the retained handles must survive");
        let trigger = EPOCH.with(|epoch| epoch.borrow().trigger_at);
        assert_eq!(trigger, 2 * owned);
    })
    .join()
    .unwrap();
}

/// Every registry the collector locks, and the allocating calls that may run a
/// collection. A guard on one of these tables held across one of those calls
/// can self-deadlock (see the `lifecycle` module doc).
const LOCKED_BY_GC: &[&str] = &[
    "FETCH_RESPONSES",
    "HEADERS_REGISTRY",
    "REQUEST_REGISTRY",
    "BLOB_REGISTRY",
    "FORM_DATA_REGISTRY",
    "FREE_FETCH_HANDLE_IDS",
    "OBJECT_URL_REGISTRY",
];
const ALLOCATES: &[&str] = &[
    "js_string_from_bytes",
    "js_array_alloc",
    "js_array_push",
    "js_class_method_bind",
    "js_promise_new",
    "js_promise_resolve",
    "js_promise_reject",
    "js_closure_alloc",
    "js_closure_call",
    "js_object_alloc",
    "js_jsvalue_to_string",
    "buffer_alloc",
    "headers_bound_method_value",
    "form_data_bound_method_value",
    "tee_readable_stream_ids",
    "throw_",
];

fn code(line: &str) -> &str {
    line.split("//").next().unwrap_or(line)
}

/// Report each guard over a `LOCKED_BY_GC` table whose scope reaches an
/// `ALLOCATES` call. Two shapes hold a guard past its statement: a named guard
/// (`let guard = T.lock()…;`, live to the end of its block or `drop(guard)`),
/// and a lock in an `if let` / `match` / `while let` / `for` head (live for the
/// whole construct).
fn guards_held_across_allocation(name: &str, text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut offenders = Vec::new();
    for (start, line) in lines.iter().enumerate() {
        let head = code(line);
        let Some(table) = LOCKED_BY_GC
            .iter()
            .find(|table| head.contains(&format!("{table}.lock()")))
        else {
            continue;
        };
        let trimmed = head.trim_start();
        let named = trimmed
            .strip_prefix("let ")
            .map(|rest| rest.trim_start_matches("mut "))
            .and_then(|rest| rest.split_once(" = "))
            .filter(|(_, rhs)| {
                // `let x = T.lock().unwrap().get(..).cloned();` drops the guard at
                // the `;`; only a binding of the guard itself outlives it.
                rhs.trim_end().ends_with(".lock().unwrap();") || rhs.trim_end().ends_with(".lock()")
            })
            .map(|(binding, _)| binding.trim().to_string());
        let block_head = ["if let ", "match ", "while let ", "for "]
            .iter()
            .any(|kw| trimmed.starts_with(kw))
            || trimmed.contains("= match ");
        if named.is_none() && !block_head {
            continue;
        }
        let mut depth = 0i32;
        let mut opened = false;
        for (offset, body) in lines[start..].iter().enumerate() {
            let body = code(body);
            if let Some(binding) = &named {
                if body.contains(&format!("drop({binding})")) {
                    break;
                }
            }
            if offset > 0 {
                if let Some(call) = ALLOCATES.iter().find(|call| body.contains(*call)) {
                    offenders.push(format!(
                        "{name}:{}: {table} guard from line {} is live at `{call}`",
                        start + offset + 1,
                        start + 1
                    ));
                }
            }
            for ch in body.chars() {
                match ch {
                    '{' => {
                        depth += 1;
                        opened = true;
                    }
                    '}' => depth -= 1,
                    _ => {}
                }
            }
            if named.is_some() && depth < 0 {
                break;
            }
            if named.is_none()
                && (opened && depth <= 0 || !opened && body.trim_end().ends_with(';'))
            {
                break;
            }
        }
    }
    offenders
}

#[test]
fn no_registry_guard_is_held_across_an_allocation() {
    const SOURCES: &[(&str, &str)] = &[
        ("fetch/mod.rs", include_str!("mod.rs")),
        ("fetch/body_clone.rs", include_str!("body_clone.rs")),
        ("fetch/body_metadata.rs", include_str!("body_metadata.rs")),
        (
            "fetch/bun_server_bridge.rs",
            include_str!("bun_server_bridge.rs"),
        ),
        ("fetch/dispatch.rs", include_str!("dispatch.rs")),
        ("fetch/gc.rs", include_str!("gc.rs")),
        ("fetch/headers.rs", include_str!("headers.rs")),
        (
            "fetch/headers_method_value.rs",
            include_str!("headers_method_value.rs"),
        ),
        ("fetch/lifecycle.rs", include_str!("lifecycle.rs")),
        ("fetch/request_ctor.rs", include_str!("request_ctor.rs")),
        ("fetch/request_handle.rs", include_str!("request_handle.rs")),
        ("fetch/response_ctor.rs", include_str!("response_ctor.rs")),
        (
            "fetch/turnloop_bridge.rs",
            include_str!("turnloop_bridge.rs"),
        ),
        ("fetch_blob.rs", include_str!("../fetch_blob.rs")),
    ];
    let offenders: Vec<String> = SOURCES
        .iter()
        .flat_map(|(name, text)| guards_held_across_allocation(name, text))
        .collect();
    assert!(
        offenders.is_empty(),
        "copy out under the guard, drop it, then allocate:\n  {}",
        offenders.join("\n  ")
    );

    // The scan must see every shape it forbids, or it is decoration. These are
    // the shapes #11165's review found (response_string_field,
    // dispatch_response_property, js_headers_get, js_response_clone).
    let planted = [
        "fn f() {\n    let guard = FETCH_RESPONSES.lock().unwrap();\n    match guard.get(&id) {\n        Some(r) => js_string_from_bytes(r.url.as_ptr(), 1),\n        None => null(),\n    }\n}\n",
        "fn f() {\n    if let Some(s) = HEADERS_REGISTRY.lock().unwrap().get(&id) {\n        return js_string_from_bytes(p, 1);\n    }\n}\n",
        "fn f() {\n    let mut guard = FETCH_RESPONSES.lock().unwrap();\n    if used {\n        throw_fetch_type_error(\"x\");\n    }\n}\n",
    ];
    for shape in planted {
        assert_eq!(
            guards_held_across_allocation("planted", shape).len(),
            1,
            "the scan no longer sees:\n{shape}"
        );
    }
    // And must not flag the fixed shape.
    let fixed = "fn f() {\n    let guard = HEADERS_REGISTRY.lock().unwrap();\n    let s = guard.len();\n    drop(guard);\n    js_string_from_bytes(p, s);\n}\n";
    assert!(guards_held_across_allocation("fixed", fixed).is_empty());
}
