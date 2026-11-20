use super::*;

struct WatchFixture {
    path: PathBuf,
}

impl WatchFixture {
    fn new() -> Self {
        crate::gc::gc_init();
        crate::object::static_shapes::js_shape_run_static_seed();
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("perry-watch-{}-{nonce}", std::process::id()));
        fs::create_dir(&path).expect("create an exclusively owned test directory");
        Self { path }
    }
}

impl Drop for WatchFixture {
    fn drop(&mut self) {
        let callback_ids: Vec<_> = FS_WATCHERS.with(|states| {
            states
                .borrow()
                .iter()
                .filter_map(|(id, state)| {
                    Path::new(&state.path)
                        .starts_with(&self.path)
                        .then_some(*id)
                })
                .collect()
        });
        for id in callback_ids {
            close_fs_watcher(id);
        }
        let promise_ids: Vec<_> = PROMISE_WATCHERS.with(|states| {
            states
                .borrow()
                .iter()
                .filter_map(|(id, state)| {
                    Path::new(&state.path)
                        .starts_with(&self.path)
                        .then_some(*id)
                })
                .collect()
        });
        for id in promise_ids {
            close_promise_watcher_return(id);
        }
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn property(value: f64, name: &str) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(value);
    let key = scope.root_string_ptr(js_string_from_bytes(name.as_ptr(), name.len() as u32));
    key.with_const_ptr(|key| {
        crate::object::js_object_get_field_by_name_f64(
            crate::value::js_nanbox_get_pointer(receiver.get_nanbox_f64())
                as *const crate::object::ObjectHeader,
            key,
        )
    })
}

fn iterator_call(iterator: &crate::gc::RuntimeHandle<'_>, name: &str) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let method = scope.root_nanbox_f64(property(iterator.get_nanbox_f64(), name));
    crate::closure::js_closure_call0(
        extract_closure_ptr(method.get_nanbox_f64()),
        crate::closure::JsThis::from_f64(iterator.get_nanbox_f64()),
    )
}

fn promise_ptr(value: f64) -> *mut crate::promise::Promise {
    crate::value::JSValue::from_bits(value.to_bits()).as_pointer::<crate::promise::Promise>()
        as *mut _
}

fn signal_iterator(path: &Path, signal: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let signal = scope.root_nanbox_f64(signal);
    let path = scope.root_nanbox_f64(string_value(path.to_string_lossy().as_bytes()));
    let options = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
    let key = scope.root_string_ptr(js_string_from_bytes(b"signal".as_ptr(), 6));
    options.with_mut_ptr(|options| {
        key.with_const_ptr(|key| {
            crate::object::js_object_set_field_by_name(options, key, signal.get_nanbox_f64())
        })
    });
    js_fs_promises_watch(
        path.get_nanbox_f64(),
        options.with_mut_ptr(|options: *mut crate::object::ObjectHeader| boxed_ptr(options.cast())),
    )
}

fn iterator_id(iterator: &crate::gc::RuntimeHandle<'_>) -> usize {
    PROMISE_WATCHERS.with(|states| {
        states
            .borrow()
            .iter()
            .find_map(|(id, state)| {
                (state.object_value.to_bits() == iterator.get_nanbox_f64().to_bits()).then_some(*id)
            })
            .unwrap()
    })
}

fn assert_done(promise: &crate::gc::RuntimeHandle<'_>) {
    assert_eq!(
        crate::promise::js_promise_state(promise_ptr(promise.get_nanbox_f64())),
        1
    );
    let scope = crate::gc::RuntimeHandleScope::new();
    let result = scope.root_nanbox_f64(crate::promise::js_promise_value(promise_ptr(
        promise.get_nanbox_f64(),
    )));
    assert_eq!(
        property(result.get_nanbox_f64(), "done").to_bits(),
        crate::value::TAG_TRUE
    );
}

fn assert_abort(promise: &crate::gc::RuntimeHandle<'_>, cause: &crate::gc::RuntimeHandle<'_>) {
    crate::promise::js_promise_mark_internally_handled(promise_ptr(promise.get_nanbox_f64()));
    assert_eq!(
        crate::promise::js_promise_state(promise_ptr(promise.get_nanbox_f64())),
        2
    );
    let scope = crate::gc::RuntimeHandleScope::new();
    let error = scope.root_nanbox_f64(crate::promise::js_promise_reason(promise_ptr(
        promise.get_nanbox_f64(),
    )));
    assert_eq!(
        read_string_value(property(error.get_nanbox_f64(), "name")).as_deref(),
        Some("AbortError")
    );
    assert_eq!(
        read_string_value(property(error.get_nanbox_f64(), "code")).as_deref(),
        Some("ABORT_ERR")
    );
    assert_eq!(
        property(error.get_nanbox_f64(), "cause").to_bits(),
        cause.get_nanbox_f64().to_bits()
    );
}

#[test]
fn abort_rejects_first_pending_next_then_finishes_the_iterator() {
    let fixture = WatchFixture::new();
    let scope = crate::gc::RuntimeHandleScope::new();
    let controller = scope.root_raw_mut_ptr(crate::url::js_abort_controller_new());
    let signal = scope.root_raw_mut_ptr(
        controller.with_mut_ptr(|controller| crate::url::js_abort_controller_signal(controller)),
    );
    let iterator = scope.root_nanbox_f64(signal_iterator(
        &fixture.path,
        signal.with_mut_ptr(|p: *mut crate::object::ObjectHeader| boxed_ptr(p.cast())),
    ));
    let first = scope.root_nanbox_f64(iterator_call(&iterator, "next"));
    let second = scope.root_nanbox_f64(iterator_call(&iterator, "next"));
    let id = iterator_id(&iterator);
    PROMISE_WATCHERS.with(|states| {
        let states = states.borrow();
        let state = &states[&id];
        assert!(state.active && state.backend.is_some() && state.persistent);
        assert_eq!(
            state.pending.len(),
            2,
            "both public next calls must actually wait"
        );
    });
    let cause = scope.root_nanbox_f64(string_value(b"owned abort cause"));
    controller.with_mut_ptr(|controller| {
        crate::url::js_abort_controller_abort_reason(controller, cause.get_nanbox_f64())
    });
    assert_abort(&first, &cause);
    assert_done(&second);
    let future = scope.root_nanbox_f64(iterator_call(&iterator, "next"));
    assert_done(&future);
    PROMISE_WATCHERS.with(|states| {
        let states = states.borrow();
        let state = &states[&id];
        assert!(state.closed && !state.active && state.backend.is_none());
        assert!(state.pending.is_empty() && state.queue.is_empty());
    });
}

#[test]
fn abort_discards_live_queued_events_and_preaborted_error_is_consumed_once() {
    let fixture = WatchFixture::new();
    let scope = crate::gc::RuntimeHandleScope::new();
    let controller = scope.root_raw_mut_ptr(crate::url::js_abort_controller_new());
    let signal = scope.root_raw_mut_ptr(
        controller.with_mut_ptr(|controller| crate::url::js_abort_controller_signal(controller)),
    );
    let signal_value =
        || signal.with_mut_ptr(|p: *mut crate::object::ObjectHeader| boxed_ptr(p.cast()));
    let iterator = scope.root_nanbox_f64(signal_iterator(&fixture.path, signal_value()));
    let first = scope.root_nanbox_f64(iterator_call(&iterator, "next"));
    let id = iterator_id(&iterator);
    for filename in ["delivered.txt", "queued.txt"] {
        deliver_event(
            id,
            WatchEvent {
                event_type: "change",
                filename: filename.to_owned(),
            },
        );
    }
    assert_eq!(
        crate::promise::js_promise_state(promise_ptr(first.get_nanbox_f64())),
        1
    );
    PROMISE_WATCHERS.with(|states| {
        assert_eq!(
            states.borrow()[&id].queue.len(),
            1,
            "the queue must be live before abort"
        )
    });
    let cause = scope.root_nanbox_f64(string_value(b"queued abort cause"));
    controller.with_mut_ptr(|controller| {
        crate::url::js_abort_controller_abort_reason(controller, cause.get_nanbox_f64())
    });
    PROMISE_WATCHERS.with(|states| assert!(states.borrow()[&id].queue.is_empty()));
    let next = scope.root_nanbox_f64(iterator_call(&iterator, "next"));
    assert_abort(&next, &cause);
    let future = scope.root_nanbox_f64(iterator_call(&iterator, "next"));
    assert_done(&future);
    let preaborted = scope.root_nanbox_f64(signal_iterator(&fixture.path, signal_value()));
    let next = scope.root_nanbox_f64(iterator_call(&preaborted, "next"));
    assert_abort(&next, &cause);
    let future = scope.root_nanbox_f64(iterator_call(&preaborted, "next"));
    assert_done(&future);
}

#[cfg(target_os = "macos")]
#[test]
fn coalesced_file_change_keeps_directory_rename_and_closed_targets_are_removed() {
    let fixture = WatchFixture::new();
    let file = fixture.path.join("file.txt");
    fs::write(&file, b"a").unwrap();
    let scope = crate::gc::RuntimeHandleScope::new();
    let mut ids = Vec::new();
    for path in [&fixture.path, &file] {
        let path = scope.root_nanbox_f64(string_value(path.to_string_lossy().as_bytes()));
        let watcher = scope.root_nanbox_f64(js_fs_watch(
            path.get_nanbox_f64(),
            undefined_value(),
            undefined_value(),
        ));
        ids.push(FS_WATCHERS.with(|states| {
            states
                .borrow()
                .iter()
                .find_map(|(id, state)| {
                    (state.object_value.to_bits() == watcher.get_nanbox_f64().to_bits())
                        .then_some(*id)
                })
                .unwrap()
        }));
    }
    FS_WATCHERS.with(|states| {
        let states = states.borrow();
        for id in &ids {
            assert!(
                states[id].refed && matches!(states[id].backend, Backend::Shared { .. }),
                "public registration must establish the native shared backend"
            );
        }
    });
    let path = fs::canonicalize(&file).unwrap();
    let class =
        super::super::watch_fsevents::classify_flags(0x0001_0000 | 0x0100 | 0x1000).unwrap();
    let raw = || RawEvent::Native {
        source: Source::Shared,
        path: path.clone(),
        class,
    };
    let mut deliveries = Vec::new();
    route_raw_event(raw(), &mut deliveries);
    assert_eq!(
        deliveries.len(),
        2,
        "both live registrations must receive the observation"
    );
    for (id, expected) in [(ids[0], "rename"), (ids[1], "change")] {
        assert!(deliveries.iter().any(|delivery| matches!(delivery, Delivery::Event { id: target, event } if *target == id && event.event_type == expected && event.filename == "file.txt")));
    }
    close_fs_watcher(ids[1]);
    deliveries.clear();
    route_raw_event(raw(), &mut deliveries);
    assert_eq!(deliveries.len(), 1);
    assert!(matches!(&deliveries[0], Delivery::Event { id, .. } if *id == ids[0]));
}
