use super::*;

#[test]
fn fetch_handle_ids_use_high_small_handle_range() {
    use perry_runtime::value::addr_class;
    assert!(FETCH_HANDLE_ID_START >= addr_class::COMMON_HANDLE_BAND_END);
    assert!(FETCH_HANDLE_ID_END <= addr_class::HANDLE_BAND_MAX);

    let native_id = crate::common::register_handle("native-request-marker".to_string());
    let id = alloc_fetch_handle_id();
    assert!((native_id as usize) < FETCH_HANDLE_ID_START);
    assert!((FETCH_HANDLE_ID_START..FETCH_HANDLE_ID_END).contains(&id));
    assert_ne!(native_id as usize, id);
    crate::common::drop_handle(native_id);
}

/// `string_from_header` must treat a handle-band value (a Fetch / native
/// registry id, not a `StringHeader` pointer) as "not a string" and return
/// `None` WITHOUT dereferencing it. Regression for the doctor / mcp-list
/// startup SIGSEGV: `fetch()` called with a non-string first argument (a
/// `Request`/`Headers` object) passed the bare handle id into the
/// `url_ptr` `*StringHeader` slot, and reading `(*ptr).byte_len` at `id+4`
/// dereferenced an unmapped low address.
#[test]
fn string_from_header_rejects_handle_band_ids() {
    use perry_runtime::value::addr_class;
    for &id in &[
        1usize,                                  // common native handle
        addr_class::FETCH_HANDLE_BAND_START,     // 0x40000
        addr_class::FETCH_HANDLE_BAND_START + 2, // a fetch handle id
        addr_class::HANDLE_BAND_MAX - 1,         // 0xFFFFF
    ] {
        assert!(addr_class::is_handle_band(id));
        // Must return None without dereferencing the bogus pointer.
        let r = unsafe { string_from_header(id as *const StringHeader) };
        assert!(
            r.is_none(),
            "handle-band id {id:#x} must be rejected, got {r:?}"
        );
    }
}

#[test]
fn response_constructor_copies_headers_initializer() {
    let mut source = HeadersStore::default();
    source.set("x", "a");
    let source_id = alloc_headers(source);

    let response = unsafe {
        js_response_new(
            std::ptr::null(),
            200.0,
            std::ptr::null(),
            handle_to_f64(source_id),
        )
    };
    let response_id = handle_id(response);
    let response_headers_id = handle_id(js_response_get_headers(response));
    assert_ne!(response_headers_id, source_id);

    HEADERS_REGISTRY
        .lock()
        .unwrap()
        .get_mut(&source_id)
        .unwrap()
        .set("x", "b");
    assert_eq!(
        HEADERS_REGISTRY
            .lock()
            .unwrap()
            .get(&response_headers_id)
            .and_then(|headers| headers.get("x")),
        Some("a".to_string())
    );

    HEADERS_REGISTRY
        .lock()
        .unwrap()
        .get_mut(&response_headers_id)
        .unwrap()
        .set("x", "c");
    assert_eq!(
        HEADERS_REGISTRY
            .lock()
            .unwrap()
            .get(&source_id)
            .and_then(|headers| headers.get("x")),
        Some("b".to_string())
    );
    assert_eq!(
        FETCH_RESPONSES
            .lock()
            .unwrap()
            .get(&response_id)
            .map(response_headers_snapshot)
            .and_then(|headers| headers.get("x")),
        Some("c".to_string())
    );

    FETCH_RESPONSES.lock().unwrap().remove(&response_id);
    HEADERS_REGISTRY.lock().unwrap().remove(&source_id);
    HEADERS_REGISTRY
        .lock()
        .unwrap()
        .remove(&response_headers_id);
}

/// #8163: the Headers / FormData bound-method caches and `RequestRecord::signal`
/// are heap values held in Rust tables outside the GC heap. The registered
/// scanner must EMIT them (mark) — a value it does not emit dies on the next
/// collection and the cache hands out a dangling closure.
#[test]
fn fetch_root_scanner_emits_method_caches_and_request_signal() {
    let headers_id = alloc_headers(HeadersStore::default());
    let headers_get = headers_bound_method_value(headers_id, "get");
    let form_id = alloc_fetch_handle_id();
    let form_bits: u64 = 0x7FFD_0000_0000_1230;
    dispatch::FORM_DATA_METHOD_VALUE_CACHE
        .lock()
        .unwrap()
        .insert((form_id, "get"), form_bits);
    let request = unsafe {
        js_request_new(
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            0.0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            f64::from_bits(TAG_FALSE),
            std::ptr::null(),
            f64::from_bits(TAG_UNDEFINED),
        )
    };
    let signal = js_request_get_signal(request);
    assert_ne!(
        signal.to_bits(),
        TAG_UNDEFINED,
        "a default AbortSignal is allocated"
    );

    let mut emitted = Vec::new();
    gc::scan_fetch_roots(&mut |value| emitted.push(value.to_bits()));

    assert!(
        emitted.contains(&headers_get.to_bits()),
        "cached Headers bound-method closure must be a root"
    );
    assert!(
        emitted.contains(&form_bits),
        "cached FormData bound-method closure must be a root"
    );
    assert!(
        emitted.contains(&signal.to_bits()),
        "request.signal must be a root"
    );

    dispatch::FORM_DATA_METHOD_VALUE_CACHE
        .lock()
        .unwrap()
        .remove(&(form_id, "get"));
}

/// #8163: marking alone is not enough under a moving collector — the cache
/// slot must be REWRITTEN, or the next `headers.get` read returns the
/// pre-move address (exactly what Next's `ReflectAdapter.get` hit). The
/// visitor here relocates only the slots this test planted, so parallel tests
/// sharing the process-global tables are untouched.
#[test]
fn fetch_root_scanner_rewrites_relocated_slots_in_place() {
    struct Relocate {
        targets: Vec<u64>,
    }
    impl gc::FetchRootVisitor for Relocate {
        fn visit_nanbox_f64_slot(&mut self, slot: &mut f64) {
            if self.targets.contains(&slot.to_bits()) {
                *slot = f64::from_bits(slot.to_bits() + 0x1000);
            }
        }
        fn visit_nanbox_u64_slot(&mut self, slot: &mut u64) {
            if self.targets.contains(slot) {
                *slot += 0x1000;
            }
        }
    }

    let headers_id = alloc_headers(HeadersStore::default());
    let before = headers_bound_method_value(headers_id, "entries");
    let request = unsafe {
        js_request_new(
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            0.0,
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            f64::from_bits(TAG_FALSE),
            std::ptr::null(),
            f64::from_bits(TAG_UNDEFINED),
        )
    };
    let signal_before = js_request_get_signal(request);

    gc::scan_fetch_roots_with(&mut Relocate {
        targets: vec![before.to_bits(), signal_before.to_bits()],
    });

    // The cache HIT path must hand back the relocated value, not the planted one.
    let after = headers_bound_method_value(headers_id, "entries");
    assert_eq!(after.to_bits(), before.to_bits() + 0x1000);
    assert_eq!(
        js_request_get_signal(request).to_bits(),
        signal_before.to_bits() + 0x1000
    );

    // Leave no bogus (relocated) pointers behind for other tests.
    headers_method_value::HEADERS_METHOD_VALUE_CACHE
        .lock()
        .unwrap()
        .remove(&(headers_id, "entries"));
    REQUEST_REGISTRY.lock().unwrap().remove(&handle_id(request));
}
