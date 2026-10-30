use super::*;

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
    pub(super) env: Option<Vec<(String, String)>>,
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
        let name = get_object_field_from_value(options(), "name");
        let thread_name = if perry_runtime::value::js_is_truthy(name) != 0 {
            // The string reader also accepts pointer-tagged values. Validate first
            // so an ordinary object is never interpreted as a StringHeader.
            perry_runtime::validators::validate_string(name, "options.name");
            string_value_to_string(name).unwrap_or_default()
        } else {
            // Node treats every falsy name as the default, not a type error.
            String::new()
        };
        Self {
            thread_name,
            resource_limits: resource_limits_from_options(options()),
            stdin: bool_option(options(), "stdin", false),
            stdout: bool_option(options(), "stdout", false),
            stderr: bool_option(options(), "stderr", false),
            env: env_option(options()),
            track_unmanaged_fds: bool_option(options(), "trackUnmanagedFds", true),
            // JS throws bypass Rust drops; acquire owned argv only after option getters.
            argv: if is_undefined(argv.get_nanbox_f64()) {
                None
            } else {
                Some(
                    perry_runtime::process::WorkerProcessState::serialize_argv(
                        argv.get_nanbox_f64(),
                    )
                    .unwrap_or_else(|detail| throw_data_clone_error(&detail)),
                )
            },
        }
    }
}

impl Default for WorkerOptions {
    fn default() -> Self {
        Self {
            thread_name: String::new(),
            argv: None,
            resource_limits: WorkerResourceLimits::node_default(),
            stdin: false,
            stdout: false,
            stderr: false,
            env: None,
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
        perry_runtime::exception::js_throw(123.0)
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
    let number = if js_value.is_int32() {
        js_value.as_int32() as f64
    } else if js_value.is_number() {
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

fn env_option(options: f64) -> Option<Vec<(String, String)>> {
    let env = get_object_field_from_value(options, "env");
    let env_obj = object_ptr_from_value(env)?;
    let keys = perry_runtime::object::js_object_keys(env_obj);
    if keys.is_null() {
        return Some(Vec::new());
    }
    let len = perry_runtime::array::js_array_length(keys);
    let mut out = Vec::with_capacity(len as usize);
    for i in 0..len {
        let key_value = perry_runtime::array::js_array_get_f64(keys, i);
        let Some(key) = string_value_to_string(key_value) else {
            continue;
        };
        let value = get_object_field_from_value(env, &key);
        if is_undefined(value) {
            continue;
        }
        let value = string_value_to_string(string_coerce(value)).unwrap_or_default();
        out.push((key, value));
    }
    Some(out)
}

pub(super) fn apply_worker_env(
    env: &Option<Vec<(String, String)>>,
) -> Option<Vec<(String, String)>> {
    let env = env.as_ref()?;
    let previous: Vec<(String, String)> = std::env::vars().collect();
    for (key, _) in &previous {
        std::env::remove_var(key);
    }
    for (key, value) in env {
        std::env::set_var(key, value);
    }
    Some(previous)
}

pub(super) fn restore_worker_env(previous: Option<Vec<(String, String)>>) {
    let Some(previous) = previous else {
        return;
    };
    let current: Vec<String> = std::env::vars().map(|(key, _)| key).collect();
    for key in current {
        std::env::remove_var(key);
    }
    for (key, value) in previous {
        std::env::set_var(key, value);
    }
}
