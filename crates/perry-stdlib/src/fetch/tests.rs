use super::*;

#[test]
fn fetch_json_preserves_document_key_order() {
    let _band = crate::fetch::handle_band_test_lock();
    let source = br#"{"title":"t","permission":[],"zeta":1,"10":"ten","2":"two"}"#;
    let parsed = unsafe { parse_json_body(source) }.expect("valid JSON body");
    let output =
        unsafe { perry_runtime::json::js_json_stringify(f64::from_bits(parsed.bits()), 0) };
    assert!(!output.is_null());
    let output = unsafe { perry_ffi::JsString::from_raw(output.cast::<perry_ffi::StringHeader>()) };
    let bytes = perry_ffi::read_bytes(output).expect("JSON.stringify returned a string");
    assert_eq!(
        bytes,
        br#"{"2":"two","10":"ten","title":"t","permission":[],"zeta":1}"#
    );
}

/// The turnloop client engine is the only transport `fetch` has, and it
/// hardcoded `RedirectMode::Follow` before this change — so a `RequestInit`
/// that carries a mode is not by itself evidence the mode is honoured. This
/// pins the engine translation, whose three arms are easy to swap silently.
#[test]
fn fetch_redirect_mode_maps_onto_the_engines_redirect_mode() {
    let _band = crate::fetch::handle_band_test_lock();
    use turnloop_http::client::RedirectMode;

    assert_eq!(
        turnloop_bridge::engine_redirect(FetchRedirectMode::Follow),
        RedirectMode::Follow
    );
    assert_eq!(
        turnloop_bridge::engine_redirect(FetchRedirectMode::Manual),
        RedirectMode::Manual
    );
    assert_eq!(
        turnloop_bridge::engine_redirect(FetchRedirectMode::Error),
        RedirectMode::Error
    );
}

/// #8546: Coop hosts each in-process deployment on its own dedicated Perry
/// thread. The Fetch scanner registry is thread-local, so a process-global
/// registration latch makes the first Next application safe and leaves the
/// second application's Request / Headers roots invisible to its collector.
///
/// Run the two applications sequentially so this test deterministically fails
/// with the old `Once`: app 1 consumes the process-global latch, then app 2
/// starts with an empty thread-local scanner registry and cannot register.
#[test]
fn fetch_root_scanner_registers_for_each_application_thread() {
    let _band = crate::fetch::handle_band_test_lock();
    for application in 1..=2 {
        let (before, after_first, after_second) = std::thread::spawn(|| {
            let before = perry_runtime::gc::gc_named_ffi_mutable_root_scanner_count();
            gc::ensure_gc_registered();
            let after_first = perry_runtime::gc::gc_named_ffi_mutable_root_scanner_count();
            gc::ensure_gc_registered();
            let after_second = perry_runtime::gc::gc_named_ffi_mutable_root_scanner_count();
            (before, after_first, after_second)
        })
        .join()
        .expect("application thread panicked");

        assert_eq!(
            before, 0,
            "application {application} must start with its own empty scanner registry"
        );
        assert_eq!(
            after_first, 1,
            "application {application} did not install the Fetch root scanner"
        );
        assert_eq!(
            after_second, 1,
            "application {application} registered the Fetch root scanner twice"
        );
    }
}

#[test]
fn fetch_handle_ids_use_high_small_handle_range() {
    let _band = crate::fetch::handle_band_test_lock();
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
    let _band = crate::fetch::handle_band_test_lock();
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
    let _band = crate::fetch::handle_band_test_lock();
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
    let _band = crate::fetch::handle_band_test_lock();
    let headers_id = alloc_headers(HeadersStore::default());
    let headers_get = headers_bound_method_value(headers_id, "get");
    let form_id = handle_id(body_metadata::js_form_data_new());
    let form_bits: u64 = 0x7FFD_0000_0000_1230;
    body_metadata::FORM_DATA_REGISTRY
        .lock()
        .unwrap()
        .get_mut(&form_id)
        .unwrap()
        .method_values
        .insert("get", form_bits);
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

    body_metadata::FORM_DATA_REGISTRY
        .lock()
        .unwrap()
        .remove(&form_id);
    HEADERS_REGISTRY.lock().unwrap().remove(&headers_id);
    REQUEST_REGISTRY.lock().unwrap().remove(&handle_id(request));
}

/// #8163: marking alone is not enough under a moving collector — the cache
/// slot must be REWRITTEN, or the next `headers.get` read returns the
/// pre-move address (exactly what Next's `ReflectAdapter.get` hit). The
/// visitor here relocates only the slots this test planted, so parallel tests
/// sharing the process-global tables are untouched.
#[test]
fn fetch_root_scanner_rewrites_relocated_slots_in_place() {
    let _band = crate::fetch::handle_band_test_lock();
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
    let form_id = handle_id(body_metadata::js_form_data_new());
    let form_before = dispatch::dispatch_form_data_property(form_id, "entries").unwrap();
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
        targets: vec![
            before.to_bits(),
            form_before.to_bits(),
            signal_before.to_bits(),
        ],
    });

    // The cache HIT path must hand back the relocated value, not the planted one.
    let after = headers_bound_method_value(headers_id, "entries");
    assert_eq!(after.to_bits(), before.to_bits() + 0x1000);
    assert_eq!(
        js_request_get_signal(request).to_bits(),
        signal_before.to_bits() + 0x1000
    );

    assert_eq!(
        dispatch::dispatch_form_data_property(form_id, "entries")
            .unwrap()
            .to_bits(),
        form_before.to_bits() + 0x1000
    );
    body_metadata::FORM_DATA_REGISTRY
        .lock()
        .unwrap()
        .remove(&form_id);

    // Leave no bogus (relocated) pointers behind for other tests.
    HEADERS_REGISTRY.lock().unwrap().remove(&headers_id);
    REQUEST_REGISTRY.lock().unwrap().remove(&handle_id(request));
}

/// #8163: every `Request` reader must RELEASE the `REQUEST_REGISTRY` guard
/// before it returns. The Fetch root scanner takes that same lock during a
/// collection on this thread, so a leaked guard — which is what a throw under
/// the guard produces, since the exception transport unwinds through the frame
/// without running `Drop` — silently disables the scanner and reintroduces
/// #8163's root cause. `try_lock` is the cheap witness: it fails if the guard
/// is still held, and unlike calling a reader while holding the lock it FAILS
/// rather than hangs.
#[test]
fn request_reads_release_the_registry_guard() {
    let _band = crate::fetch::handle_band_test_lock();
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
    let id = handle_id(request);
    assert!(
        REQUEST_REGISTRY.try_lock().is_ok(),
        "js_request_new leaked the registry guard"
    );

    let readers: &[(&str, &dyn Fn())] = &[
        ("js_request_get_url", &|| {
            js_request_get_url(request);
        }),
        ("js_request_get_method", &|| {
            js_request_get_method(request);
        }),
        ("js_request_get_body", &|| {
            js_request_get_body(request);
        }),
        ("js_request_input_to_url", &|| {
            js_request_input_to_url(request);
        }),
        ("js_request_get_signal", &|| {
            js_request_get_signal(request);
        }),
        ("js_request_get_destination", &|| {
            js_request_get_destination(request);
        }),
        ("js_request_clone", &|| {
            js_request_clone(request);
        }),
    ];
    for (name, run) in readers {
        run();
        assert!(
            REQUEST_REGISTRY.try_lock().is_ok(),
            "{name} left the registry guard held"
        );
    }

    // The untyped dispatcher's string arms are the twelve sites that used to
    // allocate under the guard; walk every one.
    for prop in [
        "url",
        "method",
        "destination",
        "referrer",
        "referrerPolicy",
        "mode",
        "credentials",
        "cache",
        "redirect",
        "integrity",
        "duplex",
        "body",
        "bodyUsed",
        "keepalive",
        "signal",
    ] {
        dispatch::dispatch_request_property(id, prop);
        assert!(
            REQUEST_REGISTRY.try_lock().is_ok(),
            "dispatch_request_property({prop:?}) left the registry guard held"
        );
    }

    REQUEST_REGISTRY.lock().unwrap().remove(&id);
}

/// #8163, the half `try_lock` cannot see: a reader that allocates *while* the
/// guard is live deadlocks against the root scanner instead of leaking, and a
/// test that reproduces it would hang rather than fail. The shape is
/// syntactic — `js_string_from_bytes(req.<field>.as_ptr(), …)` allocates
/// straight out of a borrow that only the `REQUEST_REGISTRY` guard keeps
/// alive — so scan for it. Post-fix every site snapshots the bytes into an
/// owned local first and allocates after dropping the guard.
#[test]
fn no_allocation_is_taken_off_a_live_registry_borrow() {
    const SOURCES: &[(&str, &str)] = &[
        ("fetch/mod.rs", include_str!("mod.rs")),
        ("fetch/dispatch.rs", include_str!("dispatch.rs")),
        ("fetch/body_metadata.rs", include_str!("body_metadata.rs")),
        ("fetch/request_ctor.rs", include_str!("request_ctor.rs")),
    ];
    // `req` is the universal binding for a borrowed `RequestRecord` in these
    // files; `b` is the one used for its `body`. `f(req)` is
    // `request_string_field`'s accessor form.
    const FORBIDDEN: &[&str] = &[
        "js_string_from_bytes(req.",
        "js_string_from_bytes(b.",
        "js_string_from_bytes(f(req)",
    ];
    let mut offenders = Vec::new();
    for (name, text) in SOURCES {
        for (lineno, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or(line);
            for pattern in FORBIDDEN {
                if code.contains(pattern) {
                    offenders.push(format!("{name}:{}: {}", lineno + 1, line.trim()));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "allocation taken directly off a REQUEST_REGISTRY borrow (#8163 — snapshot \
         the bytes, drop the guard, then allocate):\n  {}",
        offenders.join("\n  ")
    );

    // The scan must be able to see the shape it forbids, or it is decoration.
    let planted = "        let p = js_string_from_bytes(req.url.as_ptr(), req.url.len() as u32);";
    assert!(
        FORBIDDEN.iter().any(|pattern| planted.contains(pattern)),
        "the forbidden-pattern list no longer matches the shape it exists to catch"
    );
}

/// Return the `{ … }` body of `fn <name>` in `text`, brace-matched.
fn fetch_fn_body<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let start = text.find(&format!("fn {name}("))?;
    let open = start + text[start..].find('{')?;
    let mut depth = 0usize;
    for (offset, ch) in text[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[open..open + offset + 1]);
                }
            }
            _ => {}
        }
    }
    None
}

/// #8163/#8217: the `Headers` / `FormData` iteration surfaces were the last
/// unrooted holders behind the production App Route's lost responses, and the
/// holder is a **native Rust frame slot** — the shape no existing instrument can
/// see. `scripts/gc_runtime_root_holders.py` audits `static`/`thread_local!`
/// declarations; `scripts/gc_root_dominance_check.py` reads emitted LLVM IR from
/// compiled JS; `scripts/raw_handle_debt.py` counts bare reads out of handles
/// that already exist, and only inside `perry-runtime`. A raw `*const
/// ClosureHeader` hoisted out of a NaN-box into a Rust local, then reused after
/// `js_string_from_bytes` and after user JS, is invisible to all three.
///
/// The failure mode is a *stale* pointer, not a crash, so a unit test that
/// reproduced it would have to land a collection inside a specific loop
/// iteration. The shape is syntactic instead, so scan for it: each pre-fix
/// idiom below binds a heap address that a later allocation in the same function
/// can move, and each post-fix entry point opens a `RuntimeHandleScope` and
/// re-reads through it.
#[test]
fn headers_iteration_roots_every_heap_pointer_held_across_an_allocation() {
    let _band = crate::fetch::handle_band_test_lock();
    const SOURCES: &[(&str, &str)] = &[
        ("fetch/headers.rs", include_str!("headers.rs")),
        ("fetch/body_metadata.rs", include_str!("body_metadata.rs")),
    ];
    // Verbatim pre-fix bindings. `let mut arr = …; arr = push(arr, …)` handled the
    // array GROWING; it never handled the collector MOVING it.
    const FORBIDDEN: &[&str] = &[
        "let closure = cb_ptr as *const",
        "let mut arr = perry_runtime::js_array_alloc(",
        "let mut pair = perry_runtime::js_array_alloc(",
    ];
    let mut offenders = Vec::new();
    for (name, text) in SOURCES {
        for (lineno, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or(line);
            for pattern in FORBIDDEN {
                if code.contains(pattern) {
                    offenders.push(format!("{name}:{}: {}", lineno + 1, line.trim()));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "a raw heap address is bound across an allocation point (#8163/#8217 — root it \
         in a `RuntimeHandleScope` and re-read it after every call that can \
         collect):\n  {}",
        offenders.join("\n  ")
    );

    // The negative list alone is satisfiable by renaming a binding, so require the
    // root positively: every iteration entry point must open a scope.
    const MUST_OPEN_A_SCOPE: &[(&str, &str)] = &[
        ("fetch/headers.rs", "js_headers_for_each"),
        ("fetch/headers.rs", "js_headers_keys"),
        ("fetch/headers.rs", "js_headers_values"),
        ("fetch/headers.rs", "js_headers_entries"),
        ("fetch/headers.rs", "js_headers_get_set_cookie"),
        ("fetch/body_metadata.rs", "js_form_data_for_each"),
        ("fetch/body_metadata.rs", "js_form_data_entries"),
        // Backs `js_form_data_keys` / `js_form_data_values`.
        ("fetch/body_metadata.rs", "form_data_string_array"),
    ];
    for (file, func) in MUST_OPEN_A_SCOPE {
        let text = SOURCES
            .iter()
            .find(|(name, _)| name == file)
            .expect("source listed in MUST_OPEN_A_SCOPE must be in SOURCES")
            .1;
        let body = fetch_fn_body(text, func)
            .unwrap_or_else(|| panic!("{file}: `fn {func}` not found — has it been renamed?"));
        assert!(
            body.contains("RuntimeHandleScope"),
            "{file}: `{func}` holds heap addresses across allocations but opens no \
             `RuntimeHandleScope` (#8163/#8217)"
        );
    }

    // Both halves must be able to fail, or they are decoration.
    let planted_binding = "    let closure = cb_ptr as *const perry_runtime::ClosureHeader;";
    assert!(
        FORBIDDEN
            .iter()
            .any(|pattern| planted_binding.contains(pattern)),
        "the forbidden-pattern list no longer matches the shape it exists to catch"
    );
    let planted_unrooted = "pub extern \"C\" fn js_headers_for_each(h: f64) -> f64 {\n\
                            \x20   let closure = raw as *const ClosureHeader;\n}\n";
    let planted_body = fetch_fn_body(planted_unrooted, "js_headers_for_each")
        .expect("the body extractor must find a plain function");
    assert!(
        !planted_body.contains("RuntimeHandleScope"),
        "the body extractor no longer isolates a function body, so the positive \
         half of this test cannot fail"
    );
}

/// Dropping an owner must stop rooting its cached closures. Repeated reads
/// retain identity while the record is live, including after data mutation.
#[test]
fn method_caches_stop_rooting_after_handle_removal() {
    let _band = crate::fetch::handle_band_test_lock();
    for _ in 0..32 {
        let headers_id = alloc_headers(HeadersStore::default());
        let form_id = handle_id(body_metadata::js_form_data_new());
        let headers_method = headers_bound_method_value(headers_id, "entries");
        let form_method = dispatch::dispatch_form_data_property(form_id, "entries").unwrap();
        HEADERS_REGISTRY
            .lock()
            .unwrap()
            .get_mut(&headers_id)
            .unwrap()
            .set("x", "y");
        assert_eq!(
            headers_method.to_bits(),
            headers_bound_method_value(headers_id, "entries").to_bits()
        );
        assert_eq!(
            form_method.to_bits(),
            dispatch::dispatch_form_data_property(form_id, "entries")
                .unwrap()
                .to_bits()
        );
        let mut roots = Vec::new();
        gc::scan_fetch_roots(&mut |v| roots.push(v.to_bits()));
        assert!(roots.contains(&headers_method.to_bits()));
        assert!(roots.contains(&form_method.to_bits()));
        HEADERS_REGISTRY.lock().unwrap().remove(&headers_id);
        body_metadata::FORM_DATA_REGISTRY
            .lock()
            .unwrap()
            .remove(&form_id);
        roots.clear();
        gc::scan_fetch_roots(&mut |v| roots.push(v.to_bits()));
        assert!(
            !roots.contains(&headers_method.to_bits()),
            "removed Headers must release its cache root"
        );
        assert!(
            !roots.contains(&form_method.to_bits()),
            "removed FormData must release its cache root"
        );
    }
}

#[test]
fn copied_headers_bind_methods_to_the_new_handle() {
    let _band = crate::fetch::handle_band_test_lock();
    let source = alloc_headers(HeadersStore::default());
    let original = headers_bound_method_value(source, "get");
    let copy = HEADERS_REGISTRY
        .lock()
        .unwrap()
        .get(&source)
        .unwrap()
        .store
        .clone();
    let target = alloc_headers(copy);
    // Only the header list is copyable; the new record's cache starts empty.
    assert!(HEADERS_REGISTRY.lock().unwrap()[&target]
        .method_values
        .is_empty());
    let copied = headers_bound_method_value(target, "get");
    assert_ne!(original.to_bits(), copied.to_bits());
    let closure = perry_runtime::value::js_nanbox_get_pointer(copied)
        as *const perry_runtime::closure::ClosureHeader;
    assert_eq!(
        handle_id(perry_runtime::closure::js_closure_get_capture_f64(
            closure, 0
        )),
        target
    );
    HEADERS_REGISTRY.lock().unwrap().remove(&source);
    HEADERS_REGISTRY.lock().unwrap().remove(&target);
}

#[test]
fn copied_form_data_does_not_inherit_bound_methods() {
    let _band = crate::fetch::handle_band_test_lock();
    let source = handle_id(body_metadata::js_form_data_new());
    dispatch::dispatch_form_data_property(source, "get").unwrap();
    let forms = body_metadata::FORM_DATA_REGISTRY.lock().unwrap();
    let record = forms.get(&source).unwrap();
    assert_eq!(record.method_values.len(), 1);
    assert!(record.clone().method_values.is_empty());
    drop(forms);
    body_metadata::FORM_DATA_REGISTRY
        .lock()
        .unwrap()
        .remove(&source);
}
