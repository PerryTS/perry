use super::*;

thread_local! {
    static PULLS: Cell<usize> = const { Cell::new(0) };
    static COMPLETIONS: Cell<usize> = const { Cell::new(0) };
}

struct DisabledConservativeScan(Option<crate::gc::ConservativeStackScanMode>);

impl DisabledConservativeScan {
    fn new() -> Self {
        Self(crate::gc::set_conservative_stack_scan_override(Some(
            crate::gc::ConservativeStackScanMode::Disabled,
        )))
    }
}

impl Drop for DisabledConservativeScan {
    fn drop(&mut self) {
        crate::gc::set_conservative_stack_scan_override(self.0);
    }
}

fn promise_value(promise: *mut crate::promise::Promise) -> f64 {
    crate::value::js_nanbox_pointer(promise as i64)
}

fn result_value(done: bool, bytes: Option<&[u8]>) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let result = scope.root_nanbox_f64(object_value(crate::object::js_object_alloc(0, 0)));
    set_object_field_rooted(&scope, &result, b"done", bool_value(done));
    let chunk = scope.root_nanbox_f64(bytes.map_or_else(undefined_value, buffer_value_from_bytes));
    set_object_field_rooted(&scope, &result, b"value", chunk.get_nanbox_f64());
    result.get_nanbox_f64()
}

extern "C" fn controlled_next(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    let pull = PULLS.with(|pulls| {
        let pull = pulls.get() + 1;
        pulls.set(pull);
        pull
    });
    if pull == 1 {
        promise_value(crate::promise::js_promise_new())
    } else {
        assert_eq!(pull, 2, "only the data result and real EOF may be pulled");
        let scope = crate::gc::RuntimeHandleScope::new();
        let done = scope.root_nanbox_f64(result_value(true, None));
        promise_value(crate::promise::js_promise_resolved(done.get_nanbox_f64()))
    }
}

extern "C" fn completed(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    _value: f64,
) -> f64 {
    COMPLETIONS.with(|count| count.set(count.get() + 1));
    undefined_value()
}

fn tick() {
    crate::timer::js_callback_timer_tick();
    crate::promise::js_promise_run_microtasks();
}

fn pending_operation() -> (usize, [u64; 2]) {
    OPERATIONS.with(|operations| {
        let operations = operations.borrow();
        assert_eq!(operations.len(), 1, "a live deferred operation is required");
        let (&id, operation) = operations.iter().next().unwrap();
        assert!(operation.phase == Phase::WaitingNext);
        assert_ne!(
            operation.roots[PENDING].to_bits(),
            crate::value::TAG_UNDEFINED
        );
        (
            id,
            [
                operation.roots[ITERATOR].to_bits(),
                operation.roots[OPTIONS].to_bits(),
            ],
        )
    })
}

#[test]
fn pending_operation_rewrites_live_root_slots() {
    let _nursery = crate::gc::CopyingNurseryTestGuard::new(0);
    let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
    let _scan = DisabledConservativeScan::new();
    crate::gc::register_runtime_handle_root_scanner_for_tests();
    crate::gc::register_property_atom_scanner();
    crate::gc::gc_register_mutable_root_scanner(crate::object::shapes::scan_shape_table_rekey_mut);
    crate::gc::gc_register_mutable_root_scanner(crate::object::scan_shape_cache_roots_mut);
    crate::gc::gc_register_mutable_root_scanner(
        crate::object::scan_native_callable_export_roots_mut,
    );
    crate::gc::gc_register_mutable_root_scanner(crate::promise::scan_promise_roots_mut);
    crate::gc::gc_register_mutable_root_scanner(crate::timer::scan_timer_roots_mut);
    crate::gc::gc_register_mutable_root_scanner(crate::builtins::scan_queued_microtask_roots_mut);
    assert!(OPERATIONS.with(|operations| operations.borrow().is_empty()));
    let dir = std::env::temp_dir().join(format!(
        "perry-fs-pending-relocation-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let input = dir.join("input.txt");
    let bytes = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ012345";
    std::fs::write(&input, bytes).unwrap();
    let mut evidence = String::new();
    for borrowed in [false, true] {
        PULLS.with(|pulls| pulls.set(0));
        COMPLETIONS.with(|count| count.set(0));
        let output = dir.join(if borrowed {
            "borrowed.txt"
        } else {
            "owned.txt"
        });
        let outer = crate::gc::RuntimeHandleScope::new();
        let output_path = outer.root_nanbox_f64(string_value(output.to_str().unwrap().as_bytes()));
        let fd = if borrowed {
            Some(crate::fs::js_fs_open_sync(
                output_path.get_nanbox_f64(),
                string_value(b"w"),
            ))
        } else {
            None
        };
        let sink = outer.root_nanbox_f64(undefined_value());
        {
            let setup = crate::gc::RuntimeHandleScope::new();
            let iterator =
                setup.root_nanbox_f64(object_value(crate::object::js_object_alloc(0, 0)));
            let next = setup.root_raw_mut_ptr(js_closure_alloc(
                crate::fn_info!(controlled_next, 0; with_declared(0)),
                0,
            ));
            let next = setup.root_nanbox_f64(next.with_mut_ptr(|ptr: *mut ClosureHeader| {
                crate::value::js_nanbox_pointer(ptr as i64)
            }));
            set_object_field_rooted(&setup, &iterator, b"next", next.get_nanbox_f64());
            let options = setup.root_nanbox_f64(object_value(crate::object::js_object_alloc(0, 0)));
            sink.set_nanbox_f64(crate::fs::defer_write_file_promise(
                fd.unwrap_or_else(|| output_path.get_nanbox_f64()),
                iterator.get_nanbox_f64(),
                options.get_nanbox_f64(),
            ));
            let observer = setup.root_raw_mut_ptr(js_closure_alloc(
                crate::fn_info!(completed, 1; with_declared(1)),
                0,
            ));
            crate::promise::js_promise_attach_handlers(
                crate::value::js_nanbox_get_pointer(sink.get_nanbox_f64())
                    as *mut crate::promise::Promise,
                observer.get_raw_const_ptr(),
                std::ptr::null(),
            );
        }
        for _ in 0..32 {
            tick();
            if PULLS.with(|pulls| pulls.get() == 1) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let (id, before) = pending_operation();
        for _ in 0..3 {
            tick();
        }
        assert_eq!(
            PULLS.with(Cell::get),
            1,
            "one unresolved next means one pending pull"
        );
        assert_eq!(COMPLETIONS.with(Cell::get), 0);
        assert_eq!(
            crate::promise::js_promise_state(crate::value::js_nanbox_get_pointer(
                sink.get_nanbox_f64()
            ) as *mut crate::promise::Promise,),
            0,
            "the sink cannot complete before the source"
        );
        assert_eq!(std::fs::read(&output).unwrap(), b"");
        assert_eq!(
            crate::fs::has_pending_path_write(output_path.get_nanbox_f64()),
            !borrowed
        );
        let moved_before = crate::gc::moved_objects_total();
        let cycles_before = crate::gc::copying_minor_cycles();
        let _ = crate::gc::gc_collect_minor();
        let moved_after = crate::gc::moved_objects_total();
        let cycles_after = crate::gc::copying_minor_cycles();
        let (after_id, after) = pending_operation();
        assert_eq!(after_id, id);
        assert!(
            moved_after > moved_before,
            "premise: objects must actually relocate"
        );
        assert!(
            cycles_after > cycles_before,
            "premise: a copying minor must actually run"
        );
        assert!(
            before[0] != after[0] || before[1] != after[1],
            "premise: an actual live ITERATOR or OPTIONS registry slot must be rewritten"
        );
        assert_eq!(PULLS.with(Cell::get), 1);
        assert_eq!(COMPLETIONS.with(Cell::get), 0);
        let result = outer.root_nanbox_f64(result_value(false, Some(bytes)));
        let pending = outer.root_nanbox_f64(
            OPERATIONS.with(|operations| operations.borrow().get(&id).unwrap().roots[PENDING]),
        );
        crate::promise::js_promise_resolve(
            crate::value::js_nanbox_get_pointer(pending.get_nanbox_f64())
                as *mut crate::promise::Promise,
            result.get_nanbox_f64(),
        );
        for _ in 0..32 {
            tick();
        }
        assert_eq!(
            crate::promise::js_promise_state(crate::value::js_nanbox_get_pointer(
                sink.get_nanbox_f64()
            ) as *mut crate::promise::Promise,),
            1
        );
        assert_eq!(PULLS.with(Cell::get), 2);
        assert_eq!(COMPLETIONS.with(Cell::get), 1);
        assert_eq!(std::fs::read(&output).unwrap(), bytes);
        assert_eq!(std::fs::read(&input).unwrap(), bytes);
        assert!(OPERATIONS.with(|operations| operations.borrow().is_empty()));
        assert!(!crate::fs::has_pending_path_write(
            output_path.get_nanbox_f64()
        ));
        if let Some(fd) = fd {
            assert!(crate::fs::fd_is_registered(fd as i32));
            assert!(object_ptr_from_value(crate::fs::js_fs_fstat_sync(fd)).is_some());
            assert_eq!(crate::fs::js_fs_close_sync(fd), 1);
        }
        for _ in 0..3 {
            tick();
        }
        assert_eq!(COMPLETIONS.with(Cell::get), 1);
        evidence.push_str(&format!(
            "borrowed={borrowed} waiting_next=true pulls_before=1 before={before:x?} after={after:x?} moved_delta={} copying_delta={} bytes={} completion=1 registry_released=true path_released=true\n",
            moved_after - moved_before, cycles_after - cycles_before, bytes.len(),
        ));
    }
    std::fs::write(dir.join("proof.txt"), evidence).unwrap();
}
