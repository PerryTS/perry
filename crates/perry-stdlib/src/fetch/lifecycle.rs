//! Numeric Fetch handles are weak owners during full-heap traces. Minors
//! retain their heap edges because an old JS owner need not be visited.
//! Registry ids stay ABI-compatible and are recycled only after a full trace
//! proves them unreachable. Each mutator sweeps only the ids it allocated.
use super::*;
use std::cell::RefCell;
use std::collections::HashSet;
use std::ffi::c_void;

type Mark = extern "C" fn(u64, *mut c_void);
extern "C" {
    fn perry_ffi_gc_request_handle_collection();
    fn perry_ffi_gc_register_fetch_trace(
        phase: extern "C" fn(u32),
        observe: extern "C" fn(u64, Mark, *mut c_void) -> bool,
    );
}

thread_local! {
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
    static OWNED: RefCell<OwnedHandles> = RefCell::new(OwnedHandles::default());
    static LIVE: RefCell<Option<HashSet<usize>>> = const { RefCell::new(None) };
}

pub(super) fn register(id: usize) {
    ALLOCATIONS.with(|count| {
        let next = count.get() + 1;
        count.set(next % 4096);
        if next == 4096 {
            unsafe { perry_ffi_gc_request_handle_collection() };
        }
    });
    gc::ensure_gc_registered();
    unsafe { perry_ffi_gc_register_fetch_trace(phase, observe) };
    OWNED.with(|owned| {
        owned.borrow_mut().insert(id);
    });
    // A budgeted full trace can park while the mutator allocates. Treat new
    // entries as live until the next full, just like black GC allocations.
    LIVE.with(|live| {
        if let Some(live) = live.borrow_mut().as_mut() {
            live.insert(id);
        }
    });
}

pub(super) fn owns(id: usize) -> bool {
    OWNED
        .try_with(|owned| owned.borrow().contains(&id))
        .unwrap_or(false)
}

extern "C" fn phase(phase: u32) {
    match phase {
        0 => LIVE.with(|live| *live.borrow_mut() = Some(HashSet::new())),
        1 => {
            let live = LIVE
                .with(|live| live.borrow_mut().take())
                .unwrap_or_default();
            let dead = OWNED.with(|owned| {
                let mut owned = owned.borrow_mut();
                let dead: Vec<_> = owned.difference(&live).copied().collect();
                owned.retain(|id| live.contains(id));
                dead
            });
            release(&dead);
        }
        _ => {
            let _ = LIVE.try_with(|live| {
                live.borrow_mut().take();
            });
        }
    }
}

fn release(dead: &[usize]) {
    if dead.is_empty() {
        return;
    }
    let dead_set: HashSet<_> = dead.iter().copied().collect();
    headers_method_value::HEADERS_METHOD_VALUE_CACHE
        .lock()
        .unwrap()
        .retain(|(owner, _), _| !dead_set.contains(owner));
    dispatch::FORM_DATA_METHOD_VALUE_CACHE
        .lock()
        .unwrap()
        .retain(|(owner, _), _| !dead_set.contains(owner));
    for id in dead {
        FETCH_RESPONSES.lock().unwrap().remove(id);
        HEADERS_REGISTRY.lock().unwrap().remove(id);
        REQUEST_REGISTRY.lock().unwrap().remove(id);
        BLOB_REGISTRY.lock().unwrap().remove(id);
        body_metadata::remove_form_data(*id);
    }
    FREE_FETCH_HANDLE_IDS
        .lock()
        .unwrap()
        .extend_from_slice(dead);
}

// Thread exit is also an ownership boundary. No JS value can legally refer
// into another mutator's heap; teardown must not leave its native records or
// cached pointers in process-global tables. This Drop uses no other TLS.
#[derive(Default)]
struct OwnedHandles(HashSet<usize>);
impl std::ops::Deref for OwnedHandles {
    type Target = HashSet<usize>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for OwnedHandles {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl Drop for OwnedHandles {
    fn drop(&mut self) {
        release(&self.0.iter().copied().collect::<Vec<_>>());
    }
}

extern "C" fn observe(bits: u64, mark: Mark, ctx: *mut c_void) -> bool {
    let id = if bits >> 48 == 0x7FFD {
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if bits >> 48 == 0 {
        // Typed native-pointer slots can contain the unboxed integer id.
        bits as usize
    } else {
        return false;
    };
    if !(FETCH_HANDLE_ID_START..FETCH_HANDLE_ID_END).contains(&id) || !owns(id) {
        return false;
    }
    let first = LIVE.with(|live| {
        live.borrow_mut()
            .as_mut()
            .is_some_and(|live| live.insert(id))
    });
    if !first {
        return true;
    }
    // Snapshot before invoking the marker: an edge can cycle back through
    // another handle, and callbacks must never re-enter a held registry lock.
    let mut edges = Vec::new();
    if let Some(response) = FETCH_RESPONSES.lock().unwrap().get(&id) {
        if let Some(headers) = response.cached_headers_id {
            edges.push(handle_to_f64(headers).to_bits());
        }
    }
    if let Some(request) = REQUEST_REGISTRY.lock().unwrap().get(&id) {
        edges.push(request.signal.to_bits());
        if let Some(headers) = request.cached_headers_id {
            edges.push(handle_to_f64(headers).to_bits());
        }
    }
    body_metadata::form_data_file_edges(id, &mut edges);
    const METHODS: &[&str] = &[
        "append",
        "delete",
        "entries",
        "forEach",
        "get",
        "getAll",
        "getSetCookie",
        "has",
        "keys",
        "set",
        "values",
    ];
    for cache in [
        &*headers_method_value::HEADERS_METHOD_VALUE_CACHE,
        &*dispatch::FORM_DATA_METHOD_VALUE_CACHE,
    ] {
        let cache = cache.lock().unwrap();
        for &method in METHODS {
            if let Some(bits) = cache.get(&(id, method)) {
                edges.push(*bits);
            }
        }
    }
    for edge in edges {
        mark(edge, ctx);
    }
    true
}

/// Numeric ids do not move, so no re-read is needed after a collection.
/// Heap arguments are deliberately left to the caller's relocating scopes.
pub(super) fn pin_handles(values: &[f64]) -> perry_runtime::gc::RuntimeHandleScope {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    for &value in values {
        let id = handle_id(value);
        if (FETCH_HANDLE_ID_START..FETCH_HANDLE_ID_END).contains(&id) {
            let _ = scope.root_nanbox_f64(handle_to_f64(id));
        }
    }
    scope
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect() {
        perry_runtime::gc::js_gc_collect();
    }

    #[test]
    fn full_collection_reclaims_fetch_handles_and_retains_edges() {
        // The GC and its precise roots are per-thread. Keep this test's full
        // collections away from unrelated fixtures' unrooted Rust locals.
        std::thread::spawn(|| unsafe {
            perry_runtime::gc::gc_init();
            let scope = perry_runtime::gc::RuntimeHandleScope::new();
            let response = js_response_new(std::ptr::null(), 201.0, std::ptr::null(), 0.0);
            let root = scope.root_nanbox_f64(response);
            let headers = js_response_get_headers(response);
            let header_id = handle_id(headers);
            let method = headers_bound_method_value(header_id, "get");
            let response_id = handle_id(response);
            let dead = handle_id(js_response_new(
                std::ptr::null(),
                202.0,
                std::ptr::null(),
                0.0,
            ));
            collect();
            assert!(FETCH_RESPONSES.lock().unwrap().contains_key(&response_id));
            assert!(!FETCH_RESPONSES.lock().unwrap().contains_key(&dead));
            assert!(HEADERS_REGISTRY.lock().unwrap().contains_key(&header_id));
            assert_eq!(
                headers_bound_method_value(header_id, "get").to_bits(),
                method.to_bits()
            );
            assert_eq!(
                js_response_get_headers(root.get_nanbox_f64()).to_bits(),
                headers.to_bits()
            );
            // A detached bound method keeps its receiver alive after the
            // Response dies. The method/receiver cycle alone must not leak.
            let method_root = scope.root_nanbox_f64(method);
            root.set_nanbox_f64(f64::from_bits(TAG_UNDEFINED));
            collect();
            assert!(!FETCH_RESPONSES.lock().unwrap().contains_key(&response_id));
            assert!(HEADERS_REGISTRY.lock().unwrap().contains_key(&header_id));
            method_root.set_nanbox_f64(f64::from_bits(TAG_UNDEFINED));
            collect();
            assert!(!HEADERS_REGISTRY.lock().unwrap().contains_key(&header_id));
            assert!(!headers_method_value::HEADERS_METHOD_VALUE_CACHE
                .lock()
                .unwrap()
                .keys()
                .any(|(id, _)| *id == header_id));
        })
        .join()
        .unwrap();
    }

    #[test]
    fn automatic_full_traces_recycle_more_than_the_entire_handle_band() {
        std::thread::spawn(|| {
            perry_runtime::gc::gc_init();
            // Headers allocate no GC payload. Registry pressure itself must
            // arm full collection, otherwise this exhausts the numeric band.
            for _ in 0..(FETCH_HANDLE_ID_END - FETCH_HANDLE_ID_START + 8192) {
                js_headers_new();
                perry_runtime::gc::js_gc_loop_safepoint();
            }
            collect();
            assert!(OWNED.with(|owned| owned.borrow().is_empty()));
        })
        .join()
        .unwrap();
    }

    #[test]
    fn collection_does_not_sweep_another_mutators_handles() {
        let id = handle_id(js_headers_new());
        std::thread::spawn(|| {
            perry_runtime::gc::gc_init();
            js_headers_new();
            collect();
        })
        .join()
        .unwrap();
        assert!(HEADERS_REGISTRY.lock().unwrap().contains_key(&id));
    }
}

#[cfg(test)]
mod ownership_tests {
    use super::*;

    #[test]
    fn request_signal_and_form_data_files_follow_their_owners() {
        std::thread::spawn(|| unsafe {
            perry_runtime::gc::gc_init();
            let scope = perry_runtime::gc::RuntimeHandleScope::new();
            let snapshot =
                br#"{"url":"https://example.com/","method":"POST","headers":[],"body":[1,2,3]}"#;
            let request = js_bun_http_request_from_json(js_string_from_bytes(
                snapshot.as_ptr(),
                snapshot.len() as u32,
            ));
            let request_root = scope.root_nanbox_f64(request);
            let request_id = handle_id(request);
            let signal = js_request_get_signal(request);
            let headers = js_request_get_headers(request);
            let form = js_form_data_new();
            let form_root = scope.root_nanbox_f64(form);
            let blob = handle_to_f64(alloc_blob(BlobData {
                body: vec![1, 2, 3],
                content_type: "text/plain".into(),
                file_name: Some("part.txt".into()),
                last_modified_ms: Some(0.0),
            }));
            let name = js_string_from_bytes(b"part".as_ptr(), 4);
            js_form_data_append(
                form,
                f64::from_bits(JSValue::string_ptr(name).bits()),
                blob,
                f64::from_bits(TAG_UNDEFINED),
            );
            perry_runtime::gc::js_gc_collect();
            assert_eq!(js_request_get_signal(request).to_bits(), signal.to_bits());
            assert_eq!(js_request_get_headers(request).to_bits(), headers.to_bits());
            assert!(BLOB_REGISTRY.lock().unwrap().contains_key(&handle_id(blob)));
            // A separately retained signal does not keep the request alive.
            let _signal_root = scope.root_nanbox_f64(signal);
            request_root.set_nanbox_f64(f64::from_bits(TAG_UNDEFINED));
            form_root.set_nanbox_f64(f64::from_bits(TAG_UNDEFINED));
            perry_runtime::gc::js_gc_collect();
            assert!(!REQUEST_REGISTRY.lock().unwrap().contains_key(&request_id));
            assert!(!HEADERS_REGISTRY
                .lock()
                .unwrap()
                .contains_key(&handle_id(headers)));
            assert!(!body_metadata::is_registered_form_data(handle_id(form)));
            assert!(!BLOB_REGISTRY.lock().unwrap().contains_key(&handle_id(blob)));
        })
        .join()
        .unwrap();
    }
}

#[cfg(test)]
mod root_shape_tests {
    use super::*;

    #[test]
    fn heap_container_and_raw_native_slot_keep_handles_alive() {
        std::thread::spawn(|| unsafe {
            perry_runtime::gc::gc_init();
            let scope = perry_runtime::gc::RuntimeHandleScope::new();
            let boxed = js_headers_new();
            let raw = js_headers_new();
            let raw_root = scope.root_raw_mut_ptr(handle_id(raw) as *mut u8);
            let array = scope.root_raw_mut_ptr(perry_runtime::js_array_alloc(1));
            let ptr = perry_runtime::js_array_push_f64(array.get_raw_mut_ptr(), boxed);
            array.set_raw_mut_ptr(ptr);
            perry_runtime::gc::js_gc_collect();
            assert!(HEADERS_REGISTRY
                .lock()
                .unwrap()
                .contains_key(&handle_id(boxed)));
            assert!(HEADERS_REGISTRY
                .lock()
                .unwrap()
                .contains_key(&handle_id(raw)));
            array.set_raw_mut_ptr(std::ptr::null_mut::<perry_runtime::ArrayHeader>());
            raw_root.set_raw_mut_ptr(std::ptr::null_mut::<u8>());
            perry_runtime::gc::js_gc_collect();
            assert!(!HEADERS_REGISTRY
                .lock()
                .unwrap()
                .contains_key(&handle_id(boxed)));
            assert!(!HEADERS_REGISTRY
                .lock()
                .unwrap()
                .contains_key(&handle_id(raw)));
        })
        .join()
        .unwrap();
    }

    #[test]
    fn minor_keeps_registry_edges_and_thread_exit_releases_entries() {
        let (headers, method) = std::thread::spawn(|| {
            perry_runtime::gc::gc_init();
            let headers = js_headers_new();
            let id = handle_id(headers);
            let method = headers_bound_method_value(id, "get");
            perry_runtime::gc::gc_collect_minor();
            assert!(HEADERS_REGISTRY.lock().unwrap().contains_key(&id));
            let current = headers_bound_method_value(id, "get");
            assert_ne!(current.to_bits(), TAG_UNDEFINED);
            (id, method.to_bits())
        })
        .join()
        .unwrap();
        assert!(!HEADERS_REGISTRY.lock().unwrap().contains_key(&headers));
        assert!(!headers_method_value::HEADERS_METHOD_VALUE_CACHE
            .lock()
            .unwrap()
            .values()
            .any(|bits| *bits == method));
    }
}

#[cfg(test)]
#[test]
fn moving_collection_rewrites_live_method_cache_before_full_reclamation() {
    std::thread::spawn(|| {
        perry_runtime::gc::gc_init();
        let previous = perry_runtime::gc::js_gc_force_evacuation_test_override(1);
        perry_runtime::gc::js_gc_write_barriers_emitted(1);
        let frame = perry_runtime::gc::js_shadow_frame_push(0);
        struct Restore(u64, i32);
        impl Drop for Restore {
            fn drop(&mut self) {
                perry_runtime::gc::js_shadow_frame_pop(self.0);
                perry_runtime::gc::js_gc_write_barriers_emitted(0);
                perry_runtime::gc::js_gc_force_evacuation_test_override(self.1);
            }
        }
        let _restore = Restore(frame, previous);
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let headers = js_headers_new();
        let owner = scope.root_nanbox_f64(headers);
        let before = headers_bound_method_value(handle_id(headers), "get");
        let moved_before = perry_runtime::gc::moved_objects_total();
        perry_runtime::gc::js_gc_collect();
        let after = headers_bound_method_value(handle_id(owner.get_nanbox_f64()), "get");
        assert!(perry_runtime::gc::moved_objects_total() > moved_before);
        assert_ne!(
            before.to_bits(),
            after.to_bits(),
            "the cached closure must actually move"
        );
        owner.set_nanbox_f64(f64::from_bits(TAG_UNDEFINED));
        perry_runtime::gc::js_gc_collect();
        assert!(!HEADERS_REGISTRY
            .lock()
            .unwrap()
            .contains_key(&handle_id(headers)));
    })
    .join()
    .unwrap();
}
