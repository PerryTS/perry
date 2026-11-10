use super::*;

#[test]
fn worker_environment_is_private_and_installed_before_js() {
    const KEY: &str = "PERRY_WORKER_ENV_OWNERSHIP_TEST";
    std::env::remove_var(KEY);
    let state = WorkerProcessState::capture(Some(&[(KEY.into(), "private".into())]));
    std::thread::spawn(move || {
        state.install();
        let key = js_string_from_bytes(KEY.as_ptr(), KEY.len() as u32);
        let value = js_getenv(key);
        assert!(
            !value.is_null(),
            "Worker bootstrap must install its explicit environment"
        );
        assert!(
            std::env::var_os(KEY).is_none(),
            "Private Worker state must not change the OS environment"
        );
    })
    .join()
    .unwrap();
    assert!(std::env::var_os(KEY).is_none());
}

fn read(name: &str) -> Option<String> {
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let value = js_getenv(key);
    (!value.is_null()).then(|| {
        crate::validators::received::read_js_string_pub(crate::value::js_nanbox_string(
            value as i64,
        ))
    })
}

fn write(name: &str, value: &str) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let key = scope.root_string_ptr(js_string_from_bytes(name.as_ptr(), name.len() as u32));
    let value = js_string_from_bytes(value.as_ptr(), value.len() as u32);
    key.with_const_ptr(|key| js_setenv(key, crate::value::js_nanbox_string(value as i64)));
}

#[test]
fn worker_environment_copy_is_captured_before_thread_start() {
    const KEY: &str = "PERRY_WORKER_ENV_COPY_TEST";
    std::env::set_var(KEY, "construction");
    let state = WorkerProcessState::capture(None);
    std::env::set_var(KEY, "later");
    std::thread::spawn(move || {
        state.install();
        assert_eq!(read(KEY).as_deref(), Some("construction"));
        write(KEY, "child");
        assert_eq!(read(KEY).as_deref(), Some("child"));
    })
    .join()
    .unwrap();
    assert_eq!(std::env::var(KEY).unwrap(), "later");
    std::env::remove_var(KEY);
}

#[test]
fn shared_worker_environment_outlives_founder_and_copies_effective_parent() {
    const KEY: &str = "PERRY_WORKER_ENV_TREE_TEST";
    std::env::remove_var(KEY);
    let founder = WorkerProcessState::capture(Some(&[(KEY.into(), "founder".into())]));
    let (sibling, descendant, copied) = std::thread::spawn(move || {
        founder.install();
        (
            WorkerProcessState::capture_shared(),
            WorkerProcessState::capture_shared(),
            WorkerProcessState::capture(None),
        )
    })
    .join()
    .unwrap();
    std::thread::spawn(move || {
        sibling.install();
        write(KEY, "sibling");
    })
    .join()
    .unwrap();
    std::thread::spawn(move || {
        descendant.install();
        assert_eq!(read(KEY).as_deref(), Some("sibling"));
        let tree = WorkerProcessState::capture_shared();
        std::thread::spawn(move || {
            tree.install();
            write(KEY, "tree");
            assert_eq!(read(KEY).as_deref(), Some("tree"));
        })
        .join()
        .unwrap();
        assert_eq!(read(KEY).as_deref(), Some("tree"));
    })
    .join()
    .unwrap();
    std::thread::spawn(move || {
        copied.install();
        assert_eq!(read(KEY).as_deref(), Some("founder"));
    })
    .join()
    .unwrap();
    assert!(std::env::var_os(KEY).is_none());
}

#[test]
fn shared_worker_process_env_alias_reflects_external_additions_and_deletions() {
    let state = WorkerProcessState::capture(Some(&[("OLD".into(), "old".into())]));
    std::thread::spawn(move || {
        state.install();
        let scope = crate::gc::RuntimeHandleScope::new();
        let env = scope.root_nanbox_f64(js_process_env());
        let shared = WorkerProcessState::capture_shared();
        std::thread::spawn(move || {
            shared.install();
            write("NEW", "new");
            let old = js_string_from_bytes(b"OLD".as_ptr(), 3);
            js_removeenv(old);
        })
        .join()
        .unwrap();
        let new = scope.root_string_ptr(js_string_from_bytes(b"NEW".as_ptr(), 3));
        let value = new.with_const_ptr(|key| {
            crate::object::js_object_get_field_by_name(
                crate::value::js_nanbox_get_pointer(env.get_nanbox_f64()) as *const _,
                key,
            )
        });
        assert_eq!(
            crate::validators::received::read_js_string_pub(f64::from_bits(value.bits())),
            "new"
        );
        let old = js_string_from_bytes(b"OLD".as_ptr(), 3);
        let value = crate::object::js_object_get_field_by_name(
            crate::value::js_nanbox_get_pointer(env.get_nanbox_f64()) as *const _,
            old,
        );
        assert!(
            value.is_undefined(),
            "A removed backing entry must not leak a stale mirrored value"
        );
        let names = crate::object::js_object_get_own_property_names(env.get_nanbox_f64());
        let names = scope.root_nanbox_f64(names);
        assert_eq!(crate::array::arraylike_length(names.get_nanbox_f64()), 1);
        assert_eq!(
            crate::validators::received::read_js_string_pub(crate::array::arraylike_get(
                names.get_nanbox_f64(),
                0
            )),
            "NEW"
        );
        let keys = crate::object::js_object_keys_value(env.get_nanbox_f64());
        let keys = scope.root_raw_mut_ptr(keys);
        assert_eq!(
            keys.with_const_ptr::<crate::array::ArrayHeader, _>(|array| {
                crate::array::js_array_length(array)
            }),
            1
        );
        let descriptor = new.with_const_ptr::<StringHeader, _>(|key| {
            crate::object::js_object_get_own_property_descriptor(
                env.get_nanbox_f64(),
                crate::value::js_nanbox_string(key as i64),
            )
        });
        assert!(!JSValue::from_bits(descriptor.to_bits()).is_undefined());
        let key =
            new.with_const_ptr::<StringHeader, _>(|key| crate::value::js_nanbox_string(key as i64));
        assert_ne!(
            crate::value::js_is_truthy(crate::object::js_object_has_own(env.get_nanbox_f64(), key)),
            0
        );
        let value = js_string_from_bytes(b"alias".as_ptr(), 5);
        new.with_const_ptr(|key| {
            crate::object::js_object_set_field_by_name(
                crate::value::js_nanbox_get_pointer(env.get_nanbox_f64()) as *mut _,
                key,
                crate::value::js_nanbox_string(value as i64),
            )
        });
        assert_eq!(read("NEW").as_deref(), Some("alias"));
        new.with_const_ptr(|key| {
            crate::object::js_object_delete_field(
                crate::value::js_nanbox_get_pointer(env.get_nanbox_f64()) as *mut _,
                key,
            )
        });
        assert!(read("NEW").is_none());
    })
    .join()
    .unwrap();
}

#[test]
fn warm_environment_getter_preserves_identity_without_managed_allocation() {
    let scope = crate::gc::RuntimeHandleScope::new();
    let env = scope.root_nanbox_f64(js_process_env());
    crate::arena::sync_inline_arena_state();
    let before = crate::arena::allocation_totals();
    for _ in 0..8 {
        assert_eq!(js_process_env().to_bits(), env.get_nanbox_f64().to_bits());
    }
    crate::arena::sync_inline_arena_state();
    assert_eq!(
        crate::arena::allocation_totals(),
        before,
        "A warm process.env getter must not allocate managed storage"
    );
}

fn own_env_field(env: f64, name: &str) -> Option<JSValue> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let env = scope.root_nanbox_f64(env);
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    unsafe {
        crate::object::own_data_field_by_name(
            crate::value::js_nanbox_get_pointer(env.get_nanbox_f64()) as *const _,
            key,
        )
    }
}

#[test]
fn local_environment_mutations_update_only_their_cached_field_and_json_alias() {
    let state = WorkerProcessState::capture(Some(&[("KEEP".into(), "keep-value".into())]));
    std::thread::spawn(move || {
        state.install();
        let scope = crate::gc::RuntimeHandleScope::new();
        let env = scope.root_nanbox_f64(js_process_env());
        let keep =
            scope.root_nanbox_u64(own_env_field(env.get_nanbox_f64(), "KEEP").unwrap().bits());
        write("LOCAL", "set-value");
        assert_eq!(
            own_env_field(env.get_nanbox_f64(), "KEEP").unwrap().bits(),
            keep.get_nanbox_u64(),
            "A local set must not replace unrelated mirrored strings"
        );
        let local = own_env_field(env.get_nanbox_f64(), "LOCAL").unwrap();
        assert_eq!(
            crate::validators::received::read_js_string_pub(f64::from_bits(local.bits())),
            "set-value"
        );
        let json = unsafe { crate::json::js_json_stringify(env.get_nanbox_f64(), 0) };
        assert_eq!(
            crate::validators::received::read_js_string_pub(crate::value::js_nanbox_string(
                json as i64
            )),
            "{\"KEEP\":\"keep-value\",\"LOCAL\":\"set-value\"}"
        );
        let keep =
            scope.root_nanbox_u64(own_env_field(env.get_nanbox_f64(), "KEEP").unwrap().bits());
        let key = js_string_from_bytes(b"LOCAL".as_ptr(), 5);
        js_removeenv(key);
        assert!(own_env_field(env.get_nanbox_f64(), "LOCAL").is_none());
        assert_eq!(
            own_env_field(env.get_nanbox_f64(), "KEEP").unwrap().bits(),
            keep.get_nanbox_u64(),
            "A local delete must not replace unrelated mirrored strings"
        );
        let json = unsafe { crate::json::js_json_stringify(env.get_nanbox_f64(), 0) };
        assert_eq!(
            crate::validators::received::read_js_string_pub(crate::value::js_nanbox_string(
                json as i64
            )),
            "{\"KEEP\":\"keep-value\"}"
        );
    })
    .join()
    .unwrap();
}

fn own_env_descriptor(env: f64, name: &str) -> Option<crate::object::descriptors::OwnProperty> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let env = scope.root_nanbox_f64(env);
    let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    unsafe {
        crate::object::descriptors::own_property(
            env.get_nanbox_f64(),
            crate::value::js_nanbox_string(key as i64),
        )
    }
}

fn assert_env_descriptor(env: f64, name: &str, expected: Option<&str>) {
    match (own_env_descriptor(env, name), expected) {
        (Some(crate::object::descriptors::OwnProperty::Data(value, attrs)), Some(expected)) => {
            assert_eq!(
                crate::validators::received::read_js_string_pub(value),
                expected
            );
            assert!(attrs.writable() && attrs.enumerable() && attrs.configurable());
        }
        (None, None) => {}
        _ => panic!("Environment descriptor must reflect its effective backing"),
    }
}

#[test]
fn environment_descriptor_keeps_unqueried_mirrored_strings() {
    let state = WorkerProcessState::capture(Some(&[
        ("QUERY".into(), "query-value".into()),
        ("KEEP".into(), "unqueried-mirrored-value".into()),
    ]));
    std::thread::spawn(move || {
        state.install();
        let scope = crate::gc::RuntimeHandleScope::new();
        let env = scope.root_nanbox_f64(js_process_env());
        let keep =
            scope.root_nanbox_u64(own_env_field(env.get_nanbox_f64(), "KEEP").unwrap().bits());
        assert_env_descriptor(env.get_nanbox_f64(), "QUERY", Some("query-value"));
        assert_eq!(
            own_env_field(env.get_nanbox_f64(), "KEEP").unwrap().bits(),
            keep.get_nanbox_u64(),
            "Reading one environment descriptor must not rebuild another field's string"
        );
    })
    .join()
    .unwrap();
}

#[test]
fn unchanged_environment_enumeration_keeps_mirrored_strings() {
    let state =
        WorkerProcessState::capture(Some(&[("KEEP".into(), "unchanged-mirrored-value".into())]));
    std::thread::spawn(move || {
        state.install();
        let scope = crate::gc::RuntimeHandleScope::new();
        let env = scope.root_nanbox_f64(js_process_env());
        let keep =
            scope.root_nanbox_u64(own_env_field(env.get_nanbox_f64(), "KEEP").unwrap().bits());
        for operation in 0..4 {
            match operation {
                0 => {
                    crate::object::js_object_keys_value(env.get_nanbox_f64());
                }
                1 => {
                    crate::object::js_object_get_own_property_names(env.get_nanbox_f64());
                }
                2 => {
                    crate::object::js_object_values(crate::value::js_nanbox_get_pointer(
                        env.get_nanbox_f64(),
                    ) as *const _);
                }
                _ => {
                    crate::object::js_object_entries(crate::value::js_nanbox_get_pointer(
                        env.get_nanbox_f64(),
                    ) as *const _);
                }
            }
            assert_eq!(
                own_env_field(env.get_nanbox_f64(), "KEEP").unwrap().bits(),
                keep.get_nanbox_u64(),
                "Unchanged enumeration operation {operation} must retain the stored string"
            );
        }
    })
    .join()
    .unwrap();
}

#[test]
fn shared_environment_descriptors_remain_live_after_moving_collection() {
    let state = WorkerProcessState::capture(Some(&[
        ("OLD".into(), "old-value".into()),
        ("CHANGE".into(), "original-value".into()),
    ]));
    std::thread::spawn(move || {
        let _guard = crate::gc::CopyingNurseryTestGuard::new(0);
        let _triggers = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        crate::gc::register_runtime_handle_root_scanner_for_tests();
        crate::gc::gc_register_mutable_root_scanner(
            crate::process::scan_process_env_cache_roots_mut,
        );
        crate::gc::gc_register_mutable_root_scanner(crate::object::scan_shape_cache_roots_mut);
        crate::gc::gc_register_mutable_root_scanner(
            crate::object::shapes::scan_shape_table_rekey_mut,
        );
        state.install();
        let scope = crate::gc::RuntimeHandleScope::new();
        let env = scope.root_nanbox_f64(js_process_env());
        let before = env.get_nanbox_f64().to_bits();
        crate::gc::gc_collect_minor();
        assert_ne!(
            before,
            env.get_nanbox_f64().to_bits(),
            "The tested environment must actually move"
        );
        let shared = WorkerProcessState::capture_shared();
        std::thread::spawn(move || {
            shared.install();
            super::environment::set("CHANGE", "changed-value");
            super::environment::set("NEW_A", "first-new-value");
            super::environment::set("NEW_B", "second-new-value");
            super::environment::remove("OLD");
        })
        .join()
        .unwrap();
        // No query may synchronize the external additions before the local add.
        let key = js_string_from_bytes(b"NUMBER".as_ptr(), 6);
        js_setenv(key, 42.0);
        assert_env_descriptor(env.get_nanbox_f64(), "NEW_B", Some("second-new-value"));
        assert_env_descriptor(env.get_nanbox_f64(), "NEW_A", Some("first-new-value"));
        assert_env_descriptor(env.get_nanbox_f64(), "CHANGE", Some("changed-value"));
        assert_env_descriptor(env.get_nanbox_f64(), "OLD", None);
        assert_env_descriptor(env.get_nanbox_f64(), "NUMBER", Some("42"));
        let names = scope.root_nanbox_f64(crate::object::js_object_get_own_property_names(
            env.get_nanbox_f64(),
        ));
        let actual: Vec<_> = (0..crate::array::arraylike_length(names.get_nanbox_f64()))
            .map(|index| {
                crate::validators::received::read_js_string_pub(crate::array::arraylike_get(
                    names.get_nanbox_f64(),
                    index,
                ))
            })
            .collect();
        assert_eq!(actual, ["CHANGE", "NEW_A", "NEW_B", "NUMBER"]);
    })
    .join()
    .unwrap();
}

#[test]
fn shared_environment_descriptor_misses_keep_native_key_order() {
    let state = WorkerProcessState::capture(Some(&[("KEEP".into(), "kept-value".into())]));
    std::thread::spawn(move || {
        state.install();
        let scope = crate::gc::RuntimeHandleScope::new();
        let env = scope.root_nanbox_f64(js_process_env());
        let shared = WorkerProcessState::capture_shared();
        std::thread::spawn(move || {
            shared.install();
            super::environment::set("FIRST", "first-value");
            super::environment::set("SECOND", "second-value");
        })
        .join()
        .unwrap();
        assert_env_descriptor(env.get_nanbox_f64(), "SECOND", Some("second-value"));
        assert_env_descriptor(env.get_nanbox_f64(), "FIRST", Some("first-value"));
        let names = scope.root_nanbox_f64(crate::object::js_object_get_own_property_names(
            env.get_nanbox_f64(),
        ));
        let actual: Vec<_> = (0..crate::array::arraylike_length(names.get_nanbox_f64()))
            .map(|index| {
                crate::validators::received::read_js_string_pub(crate::array::arraylike_get(
                    names.get_nanbox_f64(),
                    index,
                ))
            })
            .collect();
        assert_eq!(actual, ["KEEP", "FIRST", "SECOND"]);
    })
    .join()
    .unwrap();
}

fn with_shared_environment_edits(check: fn(f64)) {
    let state = WorkerProcessState::capture(Some(&[
        ("FIRST".into(), "first".into()),
        ("SECOND".into(), "second".into()),
        ("DROP".into(), "dropped".into()),
    ]));
    std::thread::spawn(move || {
        state.install();
        let scope = crate::gc::RuntimeHandleScope::new();
        let env = scope.root_nanbox_f64(js_process_env());
        let shared = WorkerProcessState::capture_shared();
        std::thread::spawn(move || {
            shared.install();
            super::environment::remove("FIRST");
            super::environment::remove("DROP");
            super::environment::set("ADDED", "added");
            super::environment::set("FIRST", "renewed");
        })
        .join()
        .unwrap();
        check(env.get_nanbox_f64());
    })
    .join()
    .unwrap();
}

#[test]
fn shared_environment_delete_reinsert_uses_native_key_order() {
    with_shared_environment_edits(|env| {
        let scope = crate::gc::RuntimeHandleScope::new();
        let names = scope.root_nanbox_f64(crate::object::js_object_get_own_property_names(env));
        let actual: Vec<_> = (0..crate::array::arraylike_length(names.get_nanbox_f64()))
            .map(|index| {
                crate::validators::received::read_js_string_pub(crate::array::arraylike_get(
                    names.get_nanbox_f64(),
                    index,
                ))
            })
            .collect();
        assert_eq!(actual, ["SECOND", "ADDED", "FIRST"]);
    });
}

#[test]
fn shared_environment_json_observes_native_membership_values_and_order() {
    with_shared_environment_edits(|env| {
        let json = unsafe { crate::json::js_json_stringify(env, 0) };
        assert_eq!(
            crate::validators::received::read_js_string_pub(crate::value::js_nanbox_string(
                json as i64
            )),
            "{\"SECOND\":\"second\",\"ADDED\":\"added\",\"FIRST\":\"renewed\"}"
        );
    });
}

#[test]
fn shared_environment_spread_observes_native_membership_values_and_order() {
    with_shared_environment_edits(|env| {
        let scope = crate::gc::RuntimeHandleScope::new();
        let env = scope.root_nanbox_f64(env);
        let target = crate::object::js_object_alloc(0, 4);
        let target = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(target as i64));
        let result = unsafe {
            crate::object::js_object_literal_spread(target.get_nanbox_f64(), env.get_nanbox_f64())
        };
        let result = scope.root_nanbox_f64(result);
        let json = unsafe { crate::json::js_json_stringify(result.get_nanbox_f64(), 0) };
        assert_eq!(
            crate::validators::received::read_js_string_pub(crate::value::js_nanbox_string(
                json as i64
            )),
            "{\"SECOND\":\"second\",\"ADDED\":\"added\",\"FIRST\":\"renewed\"}"
        );
    });
}
