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
