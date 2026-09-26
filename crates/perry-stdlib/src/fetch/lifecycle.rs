//! Numeric Fetch handles are weak owners during full-heap traces. Minors
//! retain their heap edges because an old JS owner need not be visited.
//! Registry ids stay ABI-compatible and are recycled only after a full trace
//! proves them unreachable. Each mutator sweeps only the ids it allocated.
//!
//! **Young ids.** A native function can hold a freshly allocated id in a Rust
//! local, unpublished, across a GC-allocating call (`alloc_blob` followed by
//! `js_promise_resolve`, say). Rust frames are not precise roots, so an id
//! must survive the first full trace that begins after its birth: ids born
//! since the previous trace began are "young", and the Fetch root scanner
//! visits them as strong roots during full marking (initial root scan and
//! final remark), which also marks their edges through [`observe`]. Ids born
//! while a trace runs are never released by that trace. A native frame that
//! keeps an id unpublished across more than one full trace — a loop calling
//! back into user JS — must pin it (`pin_handles` or a handle scope).
//!
//! **Pacing.** Handle ids carry no GC payload, so registry growth alone never
//! reaches the heap's own triggers. A full trace is requested when this
//! mutator's owned-id count reaches `trigger_at`, reset after every trace to
//! twice the survivors (never below [`MIN_TRIGGER`]), so reclamation work is
//! amortised against growth rather than paid every N allocations regardless of
//! heap size. `alloc_fetch_handle_id` additionally asks for traces while the
//! process-wide band is nearly exhausted.
//!
//! **Locking.** A collection runs on the allocating thread and, through this
//! module and `super::gc`, locks `FETCH_RESPONSES`, `REQUEST_REGISTRY`,
//! `HEADERS_REGISTRY`, `BLOB_REGISTRY`, `FORM_DATA_REGISTRY`,
//! `FREE_FETCH_HANDLE_IDS` and the object-URL table. `std::sync::Mutex` is not
//! reentrant, so no site may hold one of those guards across a GC allocation
//! or a throw: copy what it needs out, drop the guard, then allocate.
//! `tests::no_registry_guard_is_held_across_an_allocation` enforces the shape.
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

/// Owned-id count below which registry growth never requests a full trace.
pub(super) const MIN_TRIGGER: usize = 4096;
/// Fresh ids left in the process-wide band below which allocation keeps
/// requesting full traces (every [`BAND_RESERVE_STRIDE`] fresh ids).
const BAND_RESERVE: usize = 65536;
const BAND_RESERVE_STRIDE: usize = 1024;

struct Epoch {
    /// Ids born since the most recent full trace began.
    born: HashSet<usize>,
    /// Ids born between the previous trace's start and the current one's.
    young: HashSet<usize>,
    /// `Some` while a full trace runs: the ids observed reachable so far.
    live: Option<HashSet<usize>>,
    /// Owned-id count at which the next full trace is requested.
    trigger_at: usize,
    /// This mutator has registered its scanner and trace hook.
    hooked: bool,
}

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod lifecycle_tests;

thread_local! {
    /// Completed full traces on this mutator; lets tests assert that the
    /// collection they arranged really ran.
    #[cfg(test)]
    static FULL_TRACES: Cell<usize> = const { Cell::new(0) };
    static OWNED: RefCell<OwnedHandles> = RefCell::new(OwnedHandles::default());
    static EPOCH: RefCell<Epoch> = RefCell::new(Epoch {
        born: HashSet::new(),
        young: HashSet::new(),
        live: None,
        trigger_at: MIN_TRIGGER,
        hooked: false,
    });
}

pub(super) fn register(id: usize) {
    if !EPOCH.with(|epoch| std::mem::replace(&mut epoch.borrow_mut().hooked, true)) {
        // Once per thread: the scanner and the trace hook are per-mutator.
        gc::ensure_gc_registered();
        unsafe { perry_ffi_gc_register_fetch_trace(phase, observe) };
    }
    let owned = OWNED.with(|owned| {
        let mut owned = owned.borrow_mut();
        owned.insert(id);
        owned.len()
    });
    let request = EPOCH.with(|epoch| {
        let mut epoch = epoch.borrow_mut();
        epoch.born.insert(id);
        if owned < epoch.trigger_at {
            return false;
        }
        // Re-arm rather than re-request on every allocation; a request that
        // no full trace answers is repeated MIN_TRIGGER ids later.
        epoch.trigger_at = owned + MIN_TRIGGER;
        true
    });
    if request {
        request_full_trace();
    }
}

/// Ask for a full trace at the next safe poll. Never collects synchronously:
/// callers may hold registry locks.
pub(super) fn request_full_trace() {
    unsafe { perry_ffi_gc_request_handle_collection() };
}

/// Called for every id taken fresh from the band (not from the free list).
pub(super) fn note_fresh_band_id(id: usize) {
    if FETCH_HANDLE_ID_END - id <= BAND_RESERVE && id % BAND_RESERVE_STRIDE == 0 {
        request_full_trace();
    }
}

pub(super) fn owns(id: usize) -> bool {
    OWNED
        .try_with(|owned| owned.borrow().contains(&id))
        .unwrap_or(false)
}

/// Young and in-trace-born ids, visited as strong roots by the Fetch scanner
/// during full marking (see the module doc).
pub(super) fn visit_young_handles<V: gc::FetchRootVisitor>(visitor: &mut V) {
    // Snapshot first: visiting re-enters `observe`, which borrows EPOCH.
    let ids: Vec<usize> = EPOCH.with(|epoch| {
        let epoch = epoch.borrow();
        epoch
            .young
            .iter()
            .chain(epoch.born.iter())
            .copied()
            .collect()
    });
    for id in ids {
        let mut slot = handle_to_f64(id);
        visitor.visit_nanbox_f64_slot(&mut slot);
    }
}

extern "C" fn phase(phase: u32) {
    match phase {
        0 => EPOCH.with(|epoch| {
            let mut epoch = epoch.borrow_mut();
            epoch.young = std::mem::take(&mut epoch.born);
            epoch.live = Some(HashSet::new());
        }),
        1 => finish_full_trace(),
        _ => {
            // Aborted: nothing was proven dead, and young ids stay young.
            let _ = EPOCH.try_with(|epoch| {
                let mut epoch = epoch.borrow_mut();
                epoch.live = None;
                let young = std::mem::take(&mut epoch.young);
                epoch.born.extend(young);
            });
        }
    }
}

fn finish_full_trace() {
    #[cfg(test)]
    FULL_TRACES.with(|count| count.set(count.get() + 1));
    let (live, young, born) = EPOCH.with(|epoch| {
        let mut epoch = epoch.borrow_mut();
        let live = epoch.live.take().unwrap_or_default();
        let young = std::mem::take(&mut epoch.young);
        (live, young, epoch.born.clone())
    });
    // Object URLs are strong roots until revoked. Blobs have no edges, so
    // excluding them here is equivalent to marking them.
    let object_urls = crate::fetch_blob::object_url_blob_ids();
    let (dead, survivors) = OWNED.with(|owned| {
        let mut owned = owned.borrow_mut();
        debug_assert!(
            young
                .iter()
                .all(|id| live.contains(id) || !owned.contains(id)),
            "the Fetch scanner must have observed every young id"
        );
        let keep = |id: &usize| {
            live.contains(id) || young.contains(id) || born.contains(id) || object_urls.contains(id)
        };
        let dead: Vec<_> = owned.iter().copied().filter(|id| !keep(id)).collect();
        owned.retain(|id| keep(id));
        (dead, owned.len())
    });
    EPOCH.with(|epoch| {
        epoch.borrow_mut().trigger_at = MIN_TRIGGER.max(survivors.saturating_mul(2));
    });
    release(&dead);
}

/// Remove dead records and recycle their ids. Dropping a Headers or FormData
/// record drops its bound-method cache with it. Allocates nothing on the GC
/// heap and takes each table's lock on its own.
fn release(dead: &[usize]) {
    if dead.is_empty() {
        return;
    }
    {
        let mut table = FETCH_RESPONSES.lock().unwrap();
        for id in dead {
            table.remove(id);
        }
    }
    {
        let mut table = HEADERS_REGISTRY.lock().unwrap();
        for id in dead {
            table.remove(id);
        }
    }
    {
        let mut table = REQUEST_REGISTRY.lock().unwrap();
        for id in dead {
            table.remove(id);
        }
    }
    {
        let mut table = BLOB_REGISTRY.lock().unwrap();
        for id in dead {
            table.remove(id);
        }
    }
    {
        let mut table = body_metadata::FORM_DATA_REGISTRY.lock().unwrap();
        for id in dead {
            table.remove(id);
        }
    }
    FREE_FETCH_HANDLE_IDS
        .lock()
        .unwrap()
        .extend_from_slice(dead);
}

// Thread exit is also an ownership boundary. No JS value can legally refer
// into another mutator's heap; teardown must not leave its native records or
// cached pointers in process-global tables. Blobs an unrevoked object URL
// still names are kept (the URL is process-wide); they are no longer owned by
// any mutator, so they stay until process exit. This Drop uses no other TLS.
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
        let object_urls = crate::fetch_blob::object_url_blob_ids();
        let dead: Vec<_> = self
            .0
            .iter()
            .copied()
            .filter(|id| !object_urls.contains(id))
            .collect();
        release(&dead);
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
    let first = EPOCH.with(|epoch| {
        epoch
            .borrow_mut()
            .live
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
    if let Some(record) = HEADERS_REGISTRY.lock().unwrap().get(&id) {
        edges.extend(record.method_values.values().copied());
    }
    body_metadata::form_data_edges(id, &mut edges);
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

    /// Two full collections: the first ages every id past its young epoch,
    /// the second decides by reachability alone (see the module doc).
    pub(super) fn collect() {
        perry_runtime::gc::js_gc_collect();
        perry_runtime::gc::js_gc_collect();
    }

    #[test]
    fn full_collection_reclaims_fetch_handles_and_retains_edges() {
        let _band = crate::fetch::handle_band_test_lock();
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
            // The bound-method cache lives in the record and died with it.
            assert!(!HEADERS_REGISTRY.lock().unwrap().contains_key(&header_id));
        })
        .join()
        .unwrap();
    }

    #[test]
    fn automatic_full_traces_recycle_more_than_the_entire_handle_band() {
        let _band = crate::fetch::handle_band_test_lock();
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
        let _band = crate::fetch::handle_band_test_lock();
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
        let _band = crate::fetch::handle_band_test_lock();
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
            super::tests::collect();
            assert_eq!(js_request_get_signal(request).to_bits(), signal.to_bits());
            assert_eq!(js_request_get_headers(request).to_bits(), headers.to_bits());
            assert!(BLOB_REGISTRY.lock().unwrap().contains_key(&handle_id(blob)));
            // A separately retained signal does not keep the request alive.
            let _signal_root = scope.root_nanbox_f64(signal);
            request_root.set_nanbox_f64(f64::from_bits(TAG_UNDEFINED));
            form_root.set_nanbox_f64(f64::from_bits(TAG_UNDEFINED));
            super::tests::collect();
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
        let _band = crate::fetch::handle_band_test_lock();
        std::thread::spawn(|| {
            perry_runtime::gc::gc_init();
            let scope = perry_runtime::gc::RuntimeHandleScope::new();
            let boxed = js_headers_new();
            let raw = js_headers_new();
            let raw_root = scope.root_raw_mut_ptr(handle_id(raw) as *mut u8);
            let array = scope.root_raw_mut_ptr(perry_runtime::js_array_alloc(1));
            let ptr = perry_runtime::js_array_push_f64(array.get_raw_mut_ptr(), boxed);
            array.set_raw_mut_ptr(ptr);
            super::tests::collect();
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
            super::tests::collect();
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
        let _band = crate::fetch::handle_band_test_lock();
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
        // Thread exit released the record, and its method cache with it.
        assert!(!HEADERS_REGISTRY.lock().unwrap().contains_key(&headers));
        let _ = method;
    }
}

#[cfg(test)]
#[test]
fn moving_collection_rewrites_live_method_cache_before_full_reclamation() {
    let _band = crate::fetch::handle_band_test_lock();
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
        tests::collect();
        assert!(!HEADERS_REGISTRY
            .lock()
            .unwrap()
            .contains_key(&handle_id(headers)));
    })
    .join()
    .unwrap();
}
