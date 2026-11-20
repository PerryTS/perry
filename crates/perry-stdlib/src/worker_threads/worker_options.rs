use super::*;

const DEFAULT_THREAD_NAME: &str = "WorkerThread";

#[derive(Clone, Copy)]
pub(super) struct WorkerResourceLimits {
    pub(super) max_young_generation_size_mb: f64,
    pub(super) max_old_generation_size_mb: f64,
    pub(super) code_range_size_mb: f64,
    pub(super) stack_size_mb: f64,
}

impl WorkerResourceLimits {
    pub(super) const fn node_default() -> Self {
        Self {
            max_young_generation_size_mb: -1.0,
            max_old_generation_size_mb: -1.0,
            code_range_size_mb: -1.0,
            stack_size_mb: 4.0,
        }
    }
}

pub(super) struct WorkerOptions {
    pub(super) thread_name: String,
    pub(super) argv: Option<SerializedValue>,
    pub(super) resource_limits: WorkerResourceLimits,
    pub(super) stdin: bool,
    pub(super) stdout: bool,
    pub(super) stderr: bool,
    pub(super) process_state: Option<perry_runtime::process::WorkerProcessState>,
    pub(super) track_unmanaged_fds: bool,
}

impl WorkerOptions {
    pub(super) fn from_value(options: f64) -> Self {
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let options = scope.root_nanbox_f64(options);
        let argv = scope.root_nanbox_f64(Self::prepare_argv(options.get_nanbox_f64()));
        Self::from_value_with_argv(options.get_nanbox_f64(), argv.get_nanbox_f64())
    }

    pub(super) fn prepare_argv(options: f64) -> f64 {
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let options = scope.root_nanbox_f64(options);
        if perry_runtime::value::js_is_truthy(get_object_field_from_value(
            options.get_nanbox_f64(),
            "argv",
        )) == 0
        {
            return js_undefined();
        }
        let value = get_object_field_from_value(options.get_nanbox_f64(), "argv");
        if perry_runtime::array::js_array_is_array(value).to_bits() != JSValue::bool(true).bits() {
            perry_runtime::validators::throw_invalid_arg_type(
                "options.argv",
                "an instance of Array",
                value,
            );
        }
        let value = scope.root_nanbox_f64(get_object_field_from_value(
            options.get_nanbox_f64(),
            "argv",
        ));
        let callback = perry_runtime::closure::js_closure_alloc(
            perry_runtime::fn_info!(argv_string, 1; with_declared(1)),
            0,
        );
        let callback =
            scope.root_nanbox_f64(perry_runtime::value::js_nanbox_pointer(callback as i64));
        perry_runtime::array::js_arraylike_map(
            value.get_nanbox_f64(),
            callback.get_nanbox_f64(),
            js_undefined(),
        )
    }

    pub(super) fn from_value_with_argv(options: f64, argv: f64) -> Self {
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let options = scope.root_nanbox_f64(options);
        let argv = scope.root_nanbox_f64(argv);
        let options = || options.get_nanbox_f64();
        if is_undefined(options()) {
            return Self::default();
        }
        // Normalize env before name, but capture an inherited snapshot only
        // after all observable option getters have finished.
        let env = scope.root_nanbox_f64(env_option(options()));
        let name = scope.root_nanbox_f64(get_object_field_from_value(options(), "name"));
        if perry_runtime::value::js_is_truthy(name.get_nanbox_f64()) != 0 {
            perry_runtime::validators::validate_string(name.get_nanbox_f64(), "options.name");
        }
        let resource_limits = resource_limits_from_options(options());
        let stdin = bool_option(options(), "stdin", false);
        let stdout = bool_option(options(), "stdout", false);
        let stderr = bool_option(options(), "stderr", false);
        let track_unmanaged_fds = bool_option(options(), "trackUnmanagedFds", true);
        // JS throws bypass Rust drops. Normalization keeps only rooted JS
        // strings until the final getter and fallible argv serialization finish.
        let argv = if is_undefined(argv.get_nanbox_f64()) {
            None
        } else {
            Some(
                perry_runtime::process::WorkerProcessState::serialize_argv(argv.get_nanbox_f64())
                    .unwrap_or_else(|detail| throw_data_clone_error(&detail)),
            )
        };
        let process_state =
            if unsafe { perry_runtime::symbol::js_is_symbol(env.get_nanbox_f64()) } != 0 {
                perry_runtime::process::WorkerProcessState::capture_shared()
            } else if is_undefined(env.get_nanbox_f64()) {
                perry_runtime::process::WorkerProcessState::capture(None)
            } else {
                let entries = normalized_env_entries(env.get_nanbox_f64());
                perry_runtime::process::WorkerProcessState::capture(Some(&entries))
            };
        Self {
            thread_name: if perry_runtime::value::js_is_truthy(name.get_nanbox_f64()) != 0 {
                string_value_to_string(name.get_nanbox_f64()).unwrap_or_default()
            } else {
                DEFAULT_THREAD_NAME.to_owned()
            },
            resource_limits,
            stdin,
            stdout,
            stderr,
            track_unmanaged_fds,
            argv,
            process_state: Some(process_state),
        }
    }
}

impl Default for WorkerOptions {
    fn default() -> Self {
        Self {
            thread_name: DEFAULT_THREAD_NAME.to_owned(),
            argv: None,
            resource_limits: WorkerResourceLimits::node_default(),
            stdin: false,
            stdout: false,
            stderr: false,
            process_state: Some(perry_runtime::process::WorkerProcessState::capture(None)),
            track_unmanaged_fds: true,
        }
    }
}

extern "C" fn argv_string(
    _closure: *const ClosureHeader,
    _this: perry_runtime::closure::JsThis,
    value: f64,
) -> f64 {
    string_coerce(value)
}

#[cfg(test)]
mod argv_ownership_tests {
    use super::*;

    extern "C" fn reject_options(
        _closure: *const ClosureHeader,
        _this: perry_runtime::closure::JsThis,
    ) -> f64 {
        perry_runtime::exception::throw_program_value(123.0)
    }

    #[test]
    fn option_getters_finish_before_owned_argv_serialization() {
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let options =
            scope.root_nanbox_f64(object_value(perry_runtime::object::js_object_alloc(0, 0)));
        let getter = perry_runtime::closure::js_closure_alloc(
            perry_runtime::fn_info!(reject_options, 0; with_declared(0)),
            0,
        );
        let getter = scope.root_nanbox_f64(perry_runtime::value::js_nanbox_pointer(getter as i64));
        let key = perry_runtime::string::js_string_from_bytes(b"resourceLimits".as_ptr(), 14);
        perry_runtime::object::js_object_define_getter(
            options.get_nanbox_f64(),
            perry_runtime::value::js_nanbox_string(key as i64),
            getter.get_nanbox_f64(),
        );
        // A callable cannot be serialized. The throwing getter must win before
        // argv acquires Rust-owned storage that a JS throw would bypass dropping.
        let argv = perry_runtime::array::js_array_alloc(1);
        let argv = scope.root_nanbox_f64(object_value(argv as *mut _));
        perry_runtime::array::js_array_push_f64(
            perry_runtime::value::js_nanbox_get_pointer(argv.get_nanbox_f64()) as *mut _,
            getter.get_nanbox_f64(),
        );
        let error = perry_runtime::exception::catch_js_throw(|| {
            WorkerOptions::from_value_with_argv(options.get_nanbox_f64(), argv.get_nanbox_f64())
        });
        assert_eq!(error.err().map(f64::to_bits), Some(123.0_f64.to_bits()));
    }
}

fn bool_option(options: f64, name: &str, default: bool) -> bool {
    let value = get_object_field_from_value(options, name);
    if is_undefined(value) {
        default
    } else {
        perry_runtime::value::js_is_truthy(value) != 0
    }
}

fn numeric_option(value: f64) -> Option<f64> {
    let js_value = JSValue::from_bits(value.to_bits());
    let number = if js_value.is_number() {
        js_value.as_number()
    } else {
        return None;
    };
    number.is_finite().then_some(number)
}

fn resource_limits_from_options(options: f64) -> WorkerResourceLimits {
    let mut limits = WorkerResourceLimits::node_default();
    let value = get_object_field_from_value(options, "resourceLimits");
    if object_ptr_from_value(value).is_none() {
        return limits;
    }
    if let Some(n) = numeric_option(get_object_field_from_value(
        value,
        "maxYoungGenerationSizeMb",
    )) {
        limits.max_young_generation_size_mb = n;
    }
    if let Some(n) = numeric_option(get_object_field_from_value(value, "maxOldGenerationSizeMb")) {
        limits.max_old_generation_size_mb = n;
    }
    if let Some(n) = numeric_option(get_object_field_from_value(value, "codeRangeSizeMb")) {
        limits.code_range_size_mb = n;
    }
    if let Some(n) = numeric_option(get_object_field_from_value(value, "stackSizeMb")) {
        limits.stack_size_mb = n;
    }
    limits
}

fn env_option(options: f64) -> f64 {
    let scope = perry_runtime::gc::RuntimeHandleScope::new();
    let env = scope.root_nanbox_f64(get_object_field_from_value(options, "env"));
    let value = JSValue::from_bits(env.get_nanbox_f64().to_bits());
    if value.is_null() || value.is_undefined() {
        return js_undefined();
    }
    if unsafe { perry_runtime::symbol::js_is_symbol(env.get_nanbox_f64()) } != 0 {
        let name = b"nodejs.worker_threads.SHARE_ENV";
        let name = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let shared = unsafe {
            perry_runtime::symbol::js_symbol_for(perry_runtime::value::js_nanbox_string(
                name as i64,
            ))
        };
        if env.get_nanbox_f64().to_bits() == shared.to_bits() {
            return env.get_nanbox_f64();
        }
    }
    // The runtime typeof classifier covers arrays, proxies and native objects,
    // and excludes functions independently of their heap representation.
    let kind = perry_runtime::builtins::js_value_typeof(env.get_nanbox_f64());
    if perry_runtime::string::with_string_value_bytes(
        perry_runtime::value::js_nanbox_string(kind as i64),
        |bytes| bytes == b"object",
    ) != Some(true)
    {
        perry_runtime::validators::throw_invalid_arg_type(
            "options.env",
            "of type object or one of undefined, null, or worker_threads.SHARE_ENV",
            env.get_nanbox_f64(),
        );
    }
    let keys = scope.root_raw_mut_ptr(perry_runtime::object::js_object_keys_value(
        env.get_nanbox_f64(),
    ));
    let len = keys.with_const_ptr::<perry_runtime::array::ArrayHeader, _>(|array| {
        perry_runtime::array::js_array_length(array)
    });
    let normalized =
        scope.root_raw_mut_ptr(perry_runtime::array::js_array_alloc(len.saturating_mul(2)));
    for index in 0..len {
        let key = keys.with_const_ptr::<perry_runtime::array::ArrayHeader, _>(|keys| {
            perry_runtime::array::js_array_get_f64(keys, index)
        });
        let key = scope.root_nanbox_f64(key);
        let key_pointer = scope
            .root_string_ptr(perry_runtime::value::js_get_string_pointer_unified(
                key.get_nanbox_f64(),
            ) as *const StringHeader);
        let value = key_pointer.with_const_ptr(|key| {
            perry_runtime::object::js_object_get_field_value(
                env.get_nanbox_f64().to_bits() as i64,
                key,
                0,
            )
        });
        if unsafe { perry_runtime::symbol::js_is_symbol(value) } != 0 {
            let message = b"Cannot convert a Symbol value to a string";
            let message = js_string_from_bytes(message.as_ptr(), message.len() as u32);
            perry_runtime::exception::throw_core(perry_runtime::error::CoreError::type_error(
                message,
            ));
        }
        let value = scope.root_string_ptr(perry_runtime::value::js_jsvalue_to_string_coerce(value));
        normalized.with_mut_ptr::<perry_runtime::array::ArrayHeader, _>(|array| {
            perry_runtime::array::js_array_push_f64(array, key.get_nanbox_f64())
        });
        normalized.with_mut_ptr::<perry_runtime::array::ArrayHeader, _>(|array| {
            value.with_const_ptr::<StringHeader, _>(|value| {
                perry_runtime::array::js_array_push_f64(
                    array,
                    perry_runtime::value::js_nanbox_string(value as i64),
                )
            })
        });
    }
    normalized.with_const_ptr::<perry_runtime::array::ArrayHeader, _>(|array| {
        perry_runtime::value::js_nanbox_pointer(array as i64)
    })
}

fn normalized_env_entries(value: f64) -> Vec<(String, String)> {
    let array = perry_runtime::value::js_nanbox_get_pointer(value)
        as *const perry_runtime::array::ArrayHeader;
    let len = perry_runtime::array::js_array_length(array);
    (0..len)
        .step_by(2)
        .map(|index| {
            let key = perry_runtime::array::js_array_get_f64(array, index);
            let value = perry_runtime::array::js_array_get_f64(array, index + 1);
            (
                perry_runtime::string::with_string_value_bytes(key, |bytes| {
                    String::from_utf8_lossy(bytes).into_owned()
                })
                .expect("normalized env key"),
                perry_runtime::string::with_string_value_bytes(value, |bytes| {
                    String::from_utf8_lossy(bytes).into_owned()
                })
                .expect("normalized env value"),
            )
        })
        .collect()
}

#[cfg(test)]
mod environment_tests {
    use super::*;

    fn set(object: f64, name: &str, value: f64) {
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let object = scope.root_nanbox_f64(object);
        let value = scope.root_nanbox_f64(value);
        let key = js_string_from_bytes(name.as_ptr(), name.len() as u32);
        perry_runtime::object::js_object_set_field_by_name(
            perry_runtime::value::js_nanbox_get_pointer(object.get_nanbox_f64()) as *mut _,
            key,
            value.get_nanbox_f64(),
        );
    }

    extern "C" fn name_changes_environment(
        _closure: *const ClosureHeader,
        _this: perry_runtime::closure::JsThis,
    ) -> f64 {
        std::env::set_var("PERRY_WORKER_NAME_SNAPSHOT", "after-name");
        js_undefined()
    }

    #[test]
    fn invalid_env_precedes_name_validation() {
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let options =
            scope.root_nanbox_f64(object_value(perry_runtime::object::js_object_alloc(0, 0)));
        set(options.get_nanbox_f64(), "env", 42.0);
        let name = object_value(perry_runtime::object::js_object_alloc(0, 0));
        set(options.get_nanbox_f64(), "name", name);
        let error = perry_runtime::exception::catch_js_throw(|| {
            WorkerOptions::from_value(options.get_nanbox_f64())
        })
        .err()
        .unwrap();
        let error = scope.root_nanbox_f64(error);
        assert_eq!(
            string_value_to_string(get_object_field_from_value(error.get_nanbox_f64(), "code"))
                .as_deref(),
            Some("ERR_INVALID_ARG_TYPE")
        );
        assert_eq!(string_value_to_string(get_object_field_from_value(error.get_nanbox_f64(), "message")).as_deref(), Some("The \"options.env\" property must be of type object or one of undefined, null, or worker_threads.SHARE_ENV. Received type number (42)"));
        let kinds = scope.root_raw_mut_ptr(perry_runtime::array::js_array_alloc(4));
        for value in [js_bool(true), unsafe {
            perry_runtime::symbol::js_symbol_new_empty()
        }] {
            kinds.with_mut_ptr::<perry_runtime::array::ArrayHeader, _>(|array| {
                perry_runtime::array::js_array_push_f64(array, value)
            });
        }
        let text = js_string_from_bytes(b"text".as_ptr(), 4);
        kinds.with_mut_ptr::<perry_runtime::array::ArrayHeader, _>(|array| {
            perry_runtime::array::js_array_push_f64(
                array,
                perry_runtime::value::js_nanbox_string(text as i64),
            )
        });
        let function = perry_runtime::closure::js_closure_alloc(
            perry_runtime::fn_info!(name_changes_environment, 0; with_declared(0)),
            0,
        );
        kinds.with_mut_ptr::<perry_runtime::array::ArrayHeader, _>(|array| {
            perry_runtime::array::js_array_push_f64(
                array,
                perry_runtime::value::js_nanbox_pointer(function as i64),
            )
        });
        for index in 0..4 {
            let value = kinds.with_const_ptr::<perry_runtime::array::ArrayHeader, _>(|array| {
                perry_runtime::array::js_array_get_f64(array, index)
            });
            set(options.get_nanbox_f64(), "env", value);
            let error = perry_runtime::exception::catch_js_throw(|| {
                WorkerOptions::from_value(options.get_nanbox_f64())
            })
            .err()
            .unwrap();
            let error = scope.root_nanbox_f64(error);
            let message = get_object_field_from_value(error.get_nanbox_f64(), "message");
            assert!(string_value_to_string(message)
                .unwrap()
                .starts_with("The \"options.env\" property"));
        }
    }

    #[test]
    fn explicit_env_normalizes_null_undefined_and_rejects_symbols() {
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let options =
            scope.root_nanbox_f64(object_value(perry_runtime::object::js_object_alloc(0, 0)));
        let env = scope.root_nanbox_f64(object_value(perry_runtime::object::js_object_alloc(0, 0)));
        set(env.get_nanbox_f64(), "N", js_null());
        set(env.get_nanbox_f64(), "U", js_undefined());
        set(options.get_nanbox_f64(), "env", env.get_nanbox_f64());
        let normalized = env_option(options.get_nanbox_f64());
        assert_eq!(
            normalized_env_entries(normalized),
            vec![
                ("N".into(), "null".into()),
                ("U".into(), "undefined".into())
            ]
        );
        let symbol = unsafe { perry_runtime::symbol::js_symbol_new_empty() };
        set(env.get_nanbox_f64(), "S", symbol);
        let error = perry_runtime::exception::catch_js_throw(|| {
            WorkerOptions::from_value(options.get_nanbox_f64())
        })
        .err()
        .unwrap();
        let error = scope.root_nanbox_f64(error);
        assert!(is_undefined(get_object_field_from_value(
            error.get_nanbox_f64(),
            "code"
        )));
        assert_eq!(
            string_value_to_string(get_object_field_from_value(
                error.get_nanbox_f64(),
                "message"
            ))
            .as_deref(),
            Some("Cannot convert a Symbol value to a string")
        );
    }

    #[test]
    fn inherited_environment_snapshot_includes_name_getter_effects() {
        std::env::set_var("PERRY_WORKER_NAME_SNAPSHOT", "before-name");
        let scope = perry_runtime::gc::RuntimeHandleScope::new();
        let options =
            scope.root_nanbox_f64(object_value(perry_runtime::object::js_object_alloc(0, 0)));
        let getter = perry_runtime::closure::js_closure_alloc(
            perry_runtime::fn_info!(name_changes_environment, 0; with_declared(0)),
            0,
        );
        let getter = scope.root_nanbox_f64(perry_runtime::value::js_nanbox_pointer(getter as i64));
        let key = js_string_from_bytes(b"name".as_ptr(), 4);
        perry_runtime::object::js_object_define_getter(
            options.get_nanbox_f64(),
            perry_runtime::value::js_nanbox_string(key as i64),
            getter.get_nanbox_f64(),
        );
        let state = WorkerOptions::from_value(options.get_nanbox_f64())
            .process_state
            .take()
            .unwrap();
        std::env::set_var("PERRY_WORKER_NAME_SNAPSHOT", "after-construction");
        std::thread::spawn(move || {
            state.install();
            let key = js_string_from_bytes(b"PERRY_WORKER_NAME_SNAPSHOT".as_ptr(), 26);
            let value = perry_runtime::process::js_getenv_value(key);
            assert_eq!(string_value_to_string(value).as_deref(), Some("after-name"));
        })
        .join()
        .unwrap();
        std::env::remove_var("PERRY_WORKER_NAME_SNAPSHOT");
    }
}
