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

#[derive(Clone)]
pub(super) struct WorkerOptions {
    pub(super) thread_name: String,
    pub(super) resource_limits: WorkerResourceLimits,
    pub(super) stdin: bool,
    pub(super) stdout: bool,
    pub(super) stderr: bool,
    pub(super) env: Option<Vec<(String, String)>>,
    pub(super) track_unmanaged_fds: bool,
}

impl WorkerOptions {
    pub(super) fn from_value(options: f64) -> Self {
        if is_undefined(options) {
            return Self::default();
        }
        let thread_name = string_value_to_string(get_object_field_from_value(options, "name"))
            .unwrap_or_default();
        Self {
            thread_name,
            resource_limits: resource_limits_from_options(options),
            stdin: bool_option(options, "stdin", false),
            stdout: bool_option(options, "stdout", false),
            stderr: bool_option(options, "stderr", false),
            env: env_option(options),
            track_unmanaged_fds: bool_option(options, "trackUnmanagedFds", true),
        }
    }
}

impl Default for WorkerOptions {
    fn default() -> Self {
        Self {
            thread_name: String::new(),
            resource_limits: WorkerResourceLimits::node_default(),
            stdin: false,
            stdout: false,
            stderr: false,
            env: None,
            track_unmanaged_fds: true,
        }
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
