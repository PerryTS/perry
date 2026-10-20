//! `process.report` diagnostic-report builders plus the related
//! `process.release` / `process.features` / `process.config` /
//! `process.allowedNodeEnvironmentFlags` value constructors. Split out of the
//! `process` trunk. Pure code move — no behavior change.

use super::*;
use crate::value::JSValue;

pub(crate) fn process_release_value() -> f64 {
    crate::object::runtime_layouts::runtime_record!(obj, RECORD_Q457_0);
    obj.set(
        0,
        JSValue::from_bits((module_string_value("node")).to_bits()),
    );
    obj.set(1, JSValue::from_bits((module_string_value("")).to_bits()));
    obj.set(2, JSValue::from_bits((module_string_value("")).to_bits()));
    #[cfg(windows)]
    obj.set(3, JSValue::from_bits((module_string_value("")).to_bits()));
    module_object_value(obj.finish())
}

pub(crate) fn process_features_value() -> f64 {
    crate::object::runtime_layouts::runtime_record!(obj, RECORD_Q458_0);
    obj.set(0, JSValue::from_bits((bool_value(false)).to_bits()));
    obj.set(1, JSValue::from_bits((bool_value(false)).to_bits()));
    obj.set(2, JSValue::from_bits((bool_value(true)).to_bits()));
    obj.set(3, JSValue::from_bits((bool_value(true)).to_bits()));
    obj.set(4, JSValue::from_bits((bool_value(true)).to_bits()));
    obj.set(5, JSValue::from_bits((bool_value(true)).to_bits()));
    obj.set(6, JSValue::from_bits((bool_value(true)).to_bits()));
    obj.set(7, JSValue::from_bits((bool_value(true)).to_bits()));
    obj.set(8, JSValue::from_bits((bool_value(false)).to_bits()));
    obj.set(9, JSValue::from_bits((bool_value(false)).to_bits()));
    obj.set(10, JSValue::from_bits((bool_value(false)).to_bits()));
    obj.set(11, JSValue::from_bits((bool_value(false)).to_bits()));
    obj.set(
        12,
        JSValue::from_bits((module_string_value("transform")).to_bits()),
    );
    module_object_value(obj.finish())
}

extern "C" fn process_report_function_get_report(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    err: f64,
) -> f64 {
    validate_report_error_arg(err);
    process_report_object("GetReport", None)
}

extern "C" fn process_report_function_write_report(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    file: f64,
    err: f64,
) -> f64 {
    let mut file_arg = file;
    let mut err_arg = err;
    let file_value = JSValue::from_bits(file_arg.to_bits());

    if !file_value.is_undefined() && !file_value.is_any_string() {
        if module_object_ptr(file_arg).is_some() {
            err_arg = file_arg;
            file_arg = undefined_value();
        } else {
            throw_report_invalid_arg_type("file", "string", file_arg);
        }
    }

    validate_report_error_arg(err_arg);

    let filename = module_value_to_string(file_arg)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(process_report_default_filename);
    // The report builder is filled by `js_runtime_install_diagnostics`, which
    // perry installs for a program that references `process.report`.
    let report_json = match crate::feature_hooks::PROCESS_REPORT_JSON.get() {
        Some(build) => build("API", Some(&filename)),
        None => String::from("{}"),
    };
    if let Err(err) = std::fs::write(&filename, report_json) {
        crate::fs::validate::throw_type_error_with_code(
            &format!("Failed to write diagnostic report to {filename}: {err}"),
            "ERR_REPORT_WRITE_FAILED",
        );
    }

    eprintln!("\nWriting Node.js report to file: {filename}");
    eprintln!("Node.js report completed");
    module_string_value(&filename)
}

fn validate_report_error_arg(value: f64) {
    let js = JSValue::from_bits(value.to_bits());
    if js.is_undefined() {
        return;
    }
    if module_object_ptr(value).is_none() {
        throw_report_invalid_arg_type("err", "object", value);
    }
}

fn throw_report_invalid_arg_type(name: &str, expected: &str, value: f64) -> ! {
    let message = format!(
        "The \"{}\" argument must be of type {}. Received {}",
        name,
        expected,
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn process_report_default_filename() -> String {
    format!("report.{}.json", crate::os::current_process_id())
}

thread_local! {
    /// The materialized `process.report` controller object.
    ///
    /// **This is a GC root, and must stay one (#7231).** Same shape as
    /// `CACHED_PERMISSION` and `CACHED_ENV`: nursery-allocated, cached
    /// forever, referenced by nothing else. Hoisted out of the function body
    /// so the scanner can reach it.
    static CACHED_REPORT: std::cell::Cell<f64> = const { std::cell::Cell::new(0.0) };
}

/// Root + rewrite the cached `process.report` controller object.
pub(crate) fn scan_report_cache_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    CACHED_REPORT.with(|cell| {
        let mut value = cell.get();
        if value != 0.0 && visitor.visit_nanbox_f64_slot(&mut value) {
            cell.set(value);
        }
    });
}

pub(crate) fn process_report_value() -> f64 {
    let cached = CACHED_REPORT.with(|c| c.get());
    if cached != 0.0 {
        return cached;
    }

    let obj = process_report_controller_object();
    CACHED_REPORT.with(|c| c.set(obj));
    obj
}

fn process_report_controller_object() -> f64 {
    crate::object::runtime_layouts::runtime_record!(obj, RECORD_Q459_0);
    obj.set(0, JSValue::from_bits((bool_value(false)).to_bits()));
    obj.set(1, JSValue::from_bits((module_string_value("")).to_bits()));
    obj.set(2, JSValue::from_bits((bool_value(false)).to_bits()));
    obj.set(3, JSValue::from_bits((bool_value(false)).to_bits()));
    obj.set(4, JSValue::from_bits((module_string_value("")).to_bits()));
    obj.set( 5, JSValue::from_bits((module_function1(
            "getReport",
            crate::fn_info!(process_report_function_get_report, 1; with_declared(1), with_length(1)),
            1,
        )).to_bits()));
    obj.set(6, JSValue::from_bits((bool_value(false)).to_bits()));
    obj.set(7, JSValue::from_bits((bool_value(false)).to_bits()));
    obj.set(8, JSValue::from_bits((bool_value(false)).to_bits()));
    obj.set(
        9,
        JSValue::from_bits((module_string_value("SIGUSR2")).to_bits()),
    );
    obj.set( 10, JSValue::from_bits((module_function2(
            "writeReport",
            crate::fn_info!(process_report_function_write_report, 2; with_declared(2), with_length(2)),
            2,
        )).to_bits()));
    module_object_value(obj.finish())
}

fn process_report_object(trigger: &str, filename: Option<&str>) -> f64 {
    crate::object::runtime_layouts::runtime_record!(obj, RECORD_Q460_0);
    obj.set(
        0,
        JSValue::from_bits((process_report_header_object(trigger, filename)).to_bits()),
    );
    obj.set(
        1,
        JSValue::from_bits((process_report_javascript_stack_object()).to_bits()),
    );
    obj.set(
        2,
        JSValue::from_bits((process_report_javascript_heap_object()).to_bits()),
    );
    obj.set(3, JSValue::from_bits((module_array_value(&[])).to_bits()));
    obj.set(
        4,
        JSValue::from_bits((process_report_resource_usage_object()).to_bits()),
    );
    obj.set(
        5,
        JSValue::from_bits((process_report_thread_resource_usage_object()).to_bits()),
    );
    obj.set(6, JSValue::from_bits((module_array_value(&[])).to_bits()));
    obj.set(7, JSValue::from_bits((module_array_value(&[])).to_bits()));
    obj.set(
        8,
        JSValue::from_bits(
            (module_object_value(unsafe {
                crate::object::runtime_layouts::perry_build_runtime_record(
                    std::ptr::addr_of!(crate::object::runtime_layouts::RECORD_PCHECKED031),
                    std::ptr::NonNull::<f64>::dangling().as_ptr(),
                )
            }))
            .to_bits(),
        ),
    );
    #[cfg(not(windows))]
    obj.set(
        9,
        JSValue::from_bits((process_report_user_limits_object()).to_bits()),
    );
    obj.set(
        if cfg!(windows) { 9 } else { 10 },
        JSValue::from_bits((module_array_value(&[])).to_bits()),
    );
    module_object_value(obj.finish())
}

fn process_report_header_object(trigger: &str, filename: Option<&str>) -> f64 {
    crate::object::runtime_layouts::runtime_record!(obj, RECORD_Q461_0);
    let now_ms = process_report_unix_time_ms();
    obj.set(0, JSValue::from_bits((5.0f64).to_bits()));
    obj.set(
        1,
        JSValue::from_bits((module_string_value("JavaScript API")).to_bits()),
    );
    obj.set(
        2,
        JSValue::from_bits((module_string_value(trigger)).to_bits()),
    );
    obj.set(
        3,
        JSValue::from_bits((module_string_value(filename.unwrap_or(""))).to_bits()),
    );
    obj.set(
        4,
        JSValue::from_bits((module_string_value(&format!("{:.0}", now_ms / 1000.0))).to_bits()),
    );
    obj.set(5, JSValue::from_bits((now_ms).to_bits()));
    obj.set(
        6,
        JSValue::from_bits((crate::os::current_process_id() as f64).to_bits()),
    );
    obj.set(7, JSValue::from_bits((0.0f64).to_bits()));
    obj.set(
        8,
        JSValue::from_bits(
            (module_string_value(&std::env::current_dir().map_or_else(
                |_| String::new(),
                |path| path.to_string_lossy().into_owned(),
            )))
            .to_bits(),
        ),
    );
    obj.set(
        9,
        JSValue::from_bits((process_report_command_line_array()).to_bits()),
    );
    obj.set(
        10,
        JSValue::from_bits((module_string_value("v22.0.0")).to_bits()),
    );
    obj.set(
        11,
        JSValue::from_bits(((std::mem::size_of::<usize>() * 8) as f64).to_bits()),
    );
    obj.set(
        12,
        JSValue::from_bits((module_string_value(node_arch_name())).to_bits()),
    );
    obj.set(
        13,
        JSValue::from_bits((module_string_value(node_platform_name())).to_bits()),
    );
    obj.set(
        14,
        JSValue::from_bits((process_report_component_versions()).to_bits()),
    );
    obj.set(15, JSValue::from_bits((process_release_value()).to_bits()));
    obj.set(
        16,
        JSValue::from_bits((module_string_value(std::env::consts::OS)).to_bits()),
    );
    obj.set(17, JSValue::from_bits((module_string_value("")).to_bits()));
    obj.set(18, JSValue::from_bits((module_string_value("")).to_bits()));
    obj.set(
        19,
        JSValue::from_bits((module_string_value(std::env::consts::ARCH)).to_bits()),
    );
    obj.set(20, JSValue::from_bits((module_string_value("")).to_bits()));
    module_object_value(obj.finish())
}

fn process_report_javascript_stack_object() -> f64 {
    crate::object::runtime_layouts::runtime_record!(obj, RECORD_Q462_0);
    obj.set(0, JSValue::from_bits((module_string_value("")).to_bits()));
    obj.set(1, JSValue::from_bits((module_array_value(&[])).to_bits()));
    obj.set(
        2,
        JSValue::from_bits(
            (module_object_value(unsafe {
                crate::object::runtime_layouts::perry_build_runtime_record(
                    std::ptr::addr_of!(crate::object::runtime_layouts::RECORD_PCHECKED031),
                    std::ptr::NonNull::<f64>::dangling().as_ptr(),
                )
            }))
            .to_bits(),
        ),
    );
    module_object_value(obj.finish())
}

fn process_report_javascript_heap_object() -> f64 {
    let mut heap_used: u64 = 0;
    let mut heap_total: u64 = 0;
    crate::arena::js_arena_stats(&mut heap_used, &mut heap_total);

    crate::object::runtime_layouts::runtime_record!(obj, RECORD_Q463_0);
    obj.set(0, JSValue::from_bits((heap_total as f64).to_bits()));
    obj.set(1, JSValue::from_bits((0.0f64).to_bits()));
    obj.set(2, JSValue::from_bits((heap_total as f64).to_bits()));
    obj.set(
        3,
        JSValue::from_bits((js_process_available_memory()).to_bits()),
    );
    obj.set(4, JSValue::from_bits((0.0f64).to_bits()));
    obj.set(5, JSValue::from_bits((0.0f64).to_bits()));
    obj.set(6, JSValue::from_bits((heap_used as f64).to_bits()));
    obj.set(
        7,
        JSValue::from_bits(
            (module_object_value(unsafe {
                crate::object::runtime_layouts::perry_build_runtime_record(
                    std::ptr::addr_of!(crate::object::runtime_layouts::RECORD_PCHECKED031),
                    std::ptr::NonNull::<f64>::dangling().as_ptr(),
                )
            }))
            .to_bits(),
        ),
    );
    module_object_value(obj.finish())
}

fn process_report_resource_usage_object() -> f64 {
    let (user, system) = read_process_cpu_micros();
    crate::object::runtime_layouts::runtime_record!(obj, RECORD_Q464_0);
    obj.set(0, JSValue::from_bits((user / 1_000_000.0).to_bits()));
    obj.set(1, JSValue::from_bits((system / 1_000_000.0).to_bits()));
    obj.set(2, JSValue::from_bits((0.0f64).to_bits()));
    obj.set(3, JSValue::from_bits((get_rss_bytes() as f64).to_bits()));
    obj.set(4, JSValue::from_bits((get_rss_bytes() as f64).to_bits()));
    obj.set(
        5,
        JSValue::from_bits(
            (module_object_value(unsafe {
                crate::object::runtime_layouts::perry_build_runtime_record(
                    std::ptr::addr_of!(crate::object::runtime_layouts::RECORD_PCHECKED031),
                    std::ptr::NonNull::<f64>::dangling().as_ptr(),
                )
            }))
            .to_bits(),
        ),
    );
    module_object_value(obj.finish())
}

fn process_report_thread_resource_usage_object() -> f64 {
    let (user, system) = read_thread_cpu_micros();
    crate::object::runtime_layouts::runtime_record!(obj, RECORD_Q465_0);
    obj.set(0, JSValue::from_bits((user / 1_000_000.0).to_bits()));
    obj.set(1, JSValue::from_bits((system / 1_000_000.0).to_bits()));
    obj.set(2, JSValue::from_bits((0.0f64).to_bits()));
    module_object_value(obj.finish())
}

#[cfg(not(windows))]
fn process_report_user_limits_object() -> f64 {
    crate::object::runtime_layouts::runtime_record!(obj, RECORD_Q466_0);
    obj.set(
        0,
        JSValue::from_bits((module_string_value("unlimited")).to_bits()),
    );
    obj.set(
        1,
        JSValue::from_bits((module_string_value("unlimited")).to_bits()),
    );
    obj.set(
        2,
        JSValue::from_bits((module_string_value("unlimited")).to_bits()),
    );
    module_object_value(obj.finish())
}

fn process_report_command_line_array() -> f64 {
    let args: Vec<String> = super::process_args_lossy().collect();
    let items = if args.is_empty() {
        vec![process_argv0_string()]
    } else {
        args
    };
    let arr = crate::array::js_array_alloc_with_length(items.len() as u32);
    for (i, item) in items.iter().enumerate() {
        crate::array::js_array_set_f64(arr, i as u32, module_string_value(item));
    }
    f64::from_bits(JSValue::array_ptr(arr).bits())
}

fn process_report_component_versions() -> f64 {
    crate::object::runtime_layouts::runtime_record!(obj, RECORD_Q467_0);
    obj.set(
        0,
        JSValue::from_bits((module_string_value("22.0.0")).to_bits()),
    );
    obj.set(
        1,
        JSValue::from_bits((module_string_value("12.4.254.21")).to_bits()),
    );
    obj.set(
        2,
        JSValue::from_bits((module_string_value("1.51.0")).to_bits()),
    );
    obj.set(
        3,
        JSValue::from_bits((module_string_value("0.4.71")).to_bits()),
    );
    module_object_value(obj.finish())
}

fn process_report_unix_time_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as f64)
        .unwrap_or(0.0)
}

pub(crate) fn process_report_json_string(trigger: &str, filename: Option<&str>) -> String {
    let args: Vec<String> = super::process_args_lossy().collect();
    let command_line = if args.is_empty() {
        vec![process_argv0_string()]
    } else {
        args
    };
    let now_ms = process_report_unix_time_ms();
    let mut heap_used: u64 = 0;
    let mut heap_total: u64 = 0;
    crate::arena::js_arena_stats(&mut heap_used, &mut heap_total);
    let (proc_user, proc_system) = read_process_cpu_micros();
    let (thread_user, thread_system) = read_thread_cpu_micros();

    #[allow(unused_mut)]
    let mut value = serde_json::json!({
        "header": {
            "reportVersion": 5,
            "event": "JavaScript API",
            "trigger": trigger,
            "filename": filename.unwrap_or(""),
            "dumpEventTime": format!("{:.0}", now_ms / 1000.0),
            "dumpEventTimeStamp": now_ms,
            "processId": crate::os::current_process_id(),
            "threadId": 0,
            "cwd": std::env::current_dir().map_or_else(
                |_| String::new(),
                |path| path.to_string_lossy().into_owned(),
            ),
            "commandLine": command_line,
            "nodejsVersion": "v22.0.0",
            "wordSize": std::mem::size_of::<usize>() * 8,
            "arch": node_arch_name(),
            "platform": node_platform_name(),
            "componentVersions": {
                "node": "22.0.0",
                "v8": "12.4.254.21",
                "uv": "1.51.0",
                "perry": "0.4.71"
            },
            "release": {
                "name": "node",
                "sourceUrl": "",
                "headersUrl": ""
            },
            "osName": std::env::consts::OS,
            "osRelease": "",
            "osVersion": "",
            "osMachine": std::env::consts::ARCH,
            "host": ""
        },
        "javascriptStack": {
            "message": "",
            "stack": [],
            "errorProperties": {}
        },
        "javascriptHeap": {
            "totalMemory": heap_total,
            "executableMemory": 0,
            "totalCommittedMemory": heap_total,
            "availableMemory": js_process_available_memory(),
            "totalGlobalHandlesMemory": 0,
            "usedGlobalHandlesMemory": 0,
            "usedMemory": heap_used,
            "heapSpaces": {}
        },
        "nativeStack": [],
        "resourceUsage": {
            "userCpuSeconds": proc_user / 1_000_000.0,
            "kernelCpuSeconds": proc_system / 1_000_000.0,
            "cpuConsumptionPercent": 0,
            "rss": get_rss_bytes(),
            "maxRss": get_rss_bytes(),
            "fsActivity": {}
        },
        "uvthreadResourceUsage": {
            "userCpuSeconds": thread_user / 1_000_000.0,
            "kernelCpuSeconds": thread_system / 1_000_000.0,
            "cpuConsumptionPercent": 0
        },
        "libuv": [],
        "workers": [],
        "environmentVariables": {},
        "userLimits": {
            "core_file_size_blocks": "unlimited",
            "data_size_kbytes": "unlimited",
            "file_size_blocks": "unlimited"
        },
        "sharedObjects": []
    });

    #[cfg(windows)]
    if let Some(value) = value.as_object_mut() {
        value.remove("userLimits");
        value["header"]["release"]["libUrl"] = serde_json::json!("");
    }
    serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string())
}

pub(crate) fn process_config_value() -> f64 {
    crate::object::runtime_layouts::runtime_record!(config, RECORD_Q468_0);
    crate::object::runtime_layouts::runtime_record!(variables, RECORD_Q468_1);
    crate::object::runtime_layouts::runtime_record!(target_defaults, RECORD_Q468_2);
    crate::object::runtime_layouts::runtime_record!(configurations, RECORD_Q468_3);

    variables.set(
        0,
        JSValue::from_bits((module_string_value(node_arch_name())).to_bits()),
    );
    variables.set(
        1,
        JSValue::from_bits((module_string_value(node_arch_name())).to_bits()),
    );
    variables.set(2, JSValue::from_bits((141.0f64).to_bits()));
    variables.set(3, JSValue::from_bits((bool_value(false)).to_bits()));
    variables.set(4, JSValue::from_bits((bool_value(true)).to_bits()));
    variables.set(5, JSValue::from_bits((bool_value(false)).to_bits()));
    variables.set(6, JSValue::from_bits((bool_value(false)).to_bits()));
    variables.set(7, JSValue::from_bits((1.0f64).to_bits()));
    variables.set(8, JSValue::from_bits((0.0f64).to_bits()));
    variables.set(9, JSValue::from_bits((module_string_value("")).to_bits()));

    target_defaults.set(0, JSValue::from_bits((module_array_value(&[])).to_bits()));
    target_defaults.set(1, JSValue::from_bits((module_array_value(&[])).to_bits()));
    target_defaults.set(2, JSValue::from_bits((module_array_value(&[])).to_bits()));
    target_defaults.set(3, JSValue::from_bits((module_array_value(&[])).to_bits()));
    target_defaults.set(4, JSValue::from_bits((module_array_value(&[])).to_bits()));
    target_defaults.set(
        5,
        JSValue::from_bits((module_string_value("Release")).to_bits()),
    );
    configurations.set(
        0,
        JSValue::from_bits(
            (module_object_value(unsafe {
                crate::object::runtime_layouts::perry_build_runtime_record(
                    std::ptr::addr_of!(crate::object::runtime_layouts::RECORD_PCHECKED031),
                    std::ptr::NonNull::<f64>::dangling().as_ptr(),
                )
            }))
            .to_bits(),
        ),
    );
    target_defaults.set(
        6,
        JSValue::from_bits((module_object_value(configurations.finish())).to_bits()),
    );

    config.set(
        0,
        JSValue::from_bits((module_object_value(variables.finish())).to_bits()),
    );
    config.set(
        1,
        JSValue::from_bits((module_object_value(target_defaults.finish())).to_bits()),
    );
    module_object_value(config.finish())
}

pub(crate) fn process_allowed_flags_value() -> f64 {
    const FLAGS: &[&str] = &[
        "--abort-on-uncaught-exception",
        "--addons",
        "--allow-addons",
        "--allow-child-process",
        "--allow-fs-read",
        "--allow-fs-write",
        "--allow-inspector",
        "--allow-net",
        "--allow-wasi",
        "--allow-worker",
        "--async-context-frame",
        "--conditions",
        "--cpu-prof",
        "--cpu-prof-dir",
        "--cpu-prof-interval",
        "--cpu-prof-name",
        "--debug-arraybuffer-allocations",
        "--debug-port",
        "--deprecation",
        "--diagnostic-dir",
        "--disable-proto",
        "--disable-sigusr1",
        "--disable-warning",
        "--disable-wasm-trap-handler",
        "--disallow-code-generation-from-strings",
        "--dns-result-order",
        "--enable-etw-stack-walking",
        "--enable-fips",
        "--enable-network-family-autoselection",
        "--enable-source-maps",
        "--entry-url",
        "--es-module-specifier-resolution",
        "--experimental-abortcontroller",
        "--experimental-addon-modules",
        "--experimental-detect-module",
        "--experimental-eventsource",
        "--experimental-fetch",
        "--experimental-global-customevent",
        "--experimental-global-navigator",
        "--experimental-global-webcrypto",
        "--experimental-import-meta-resolve",
        "--experimental-json-modules",
        "--experimental-loader",
        "--experimental-modules",
        "--experimental-print-required-tla",
        "--experimental-quic",
        "--experimental-repl-await",
        "--experimental-report",
        "--experimental-require-module",
        "--experimental-shadow-realm",
        "--experimental-specifier-resolution",
        "--experimental-sqlite",
        "--experimental-strip-types",
        "--experimental-test-isolation",
        "--experimental-top-level-await",
        "--experimental-transform-types",
        "--experimental-vm-modules",
        "--experimental-wasi-unstable-preview1",
        "--experimental-wasm-modules",
        "--experimental-websocket",
        "--experimental-webstorage",
        "--experimental-worker",
        "--expose-gc",
        "--extra-info-on-fatal-exception",
        "--force-async-hooks-checks",
        "--force-context-aware",
        "--force-fips",
        "--force-node-api-uncaught-exceptions-policy",
        "--frozen-intrinsics",
        "--global-search-paths",
        "--heap-prof",
        "--heap-prof-dir",
        "--heap-prof-interval",
        "--heap-prof-name",
        "--heapsnapshot-near-heap-limit",
        "--heapsnapshot-signal",
        "--http-parser",
        "--icu-data-dir",
        "--import",
        "--input-type",
        "--insecure-http-parser",
        "--inspect",
        "--inspect-brk",
        "--inspect-port",
        "--inspect-publish-uid",
        "--inspect-wait",
        "--interpreted-frames-native-stack",
        "--jitless",
        "--loader",
        "--localstorage-file",
        "--max-http-header-size",
        "--max-old-space-size",
        "--max-old-space-size-percentage",
        "--max-semi-space-size",
        "--napi-modules",
        "--network-family-autoselection",
        "--network-family-autoselection-attempt-timeout",
        "--no-addons",
        "--no-allow-addons",
        "--no-allow-child-process",
        "--no-allow-inspector",
        "--no-allow-net",
        "--no-allow-wasi",
        "--no-allow-worker",
        "--no-async-context-frame",
        "--no-cpu-prof",
        "--no-debug-arraybuffer-allocations",
        "--no-deprecation",
        "--no-disable-sigusr1",
        "--no-disable-wasm-trap-handler",
        "--no-enable-fips",
        "--no-enable-source-maps",
        "--no-entry-url",
        "--no-experimental-addon-modules",
        "--no-experimental-detect-module",
        "--no-experimental-eventsource",
        "--no-experimental-global-navigator",
        "--no-experimental-import-meta-resolve",
        "--no-experimental-print-required-tla",
        "--no-experimental-repl-await",
        "--no-experimental-require-module",
        "--no-experimental-shadow-realm",
        "--no-experimental-sqlite",
        "--no-experimental-transform-types",
        "--no-experimental-vm-modules",
        "--no-experimental-websocket",
        "--no-experimental-webstorage",
        "--no-extra-info-on-fatal-exception",
        "--no-force-async-hooks-checks",
        "--no-force-context-aware",
        "--no-force-fips",
        "--no-force-node-api-uncaught-exceptions-policy",
        "--no-frozen-intrinsics",
        "--no-global-search-paths",
        "--no-heap-prof",
        "--no-insecure-http-parser",
        "--no-inspect",
        "--no-inspect-brk",
        "--no-inspect-wait",
        "--no-network-family-autoselection",
        "--no-node-snapshot",
        "--no-openssl-legacy-provider",
        "--no-openssl-shared-config",
        "--no-pending-deprecation",
        "--no-permission",
        "--no-permission-audit",
        "--no-preserve-symlinks",
        "--no-preserve-symlinks-main",
        "--no-report-compact",
        "--no-report-exclude-env",
        "--no-report-exclude-network",
        "--no-report-on-fatalerror",
        "--no-report-on-signal",
        "--no-report-uncaught-exception",
        "--no-require-module",
        "--no-strip-types",
        "--no-test-only",
        "--no-throw-deprecation",
        "--no-tls-max-v1.2",
        "--no-tls-max-v1.3",
        "--no-tls-min-v1.0",
        "--no-tls-min-v1.1",
        "--no-tls-min-v1.2",
        "--no-tls-min-v1.3",
        "--no-trace-deprecation",
        "--no-trace-env",
        "--no-trace-env-js-stack",
        "--no-trace-env-native-stack",
        "--no-trace-exit",
        "--no-trace-promises",
        "--no-trace-sigint",
        "--no-trace-sync-io",
        "--no-trace-tls",
        "--no-trace-uncaught",
        "--no-trace-warnings",
        "--no-track-heap-objects",
        "--no-use-bundled-ca",
        "--no-use-env-proxy",
        "--no-use-openssl-ca",
        "--no-use-system-ca",
        "--no-verify-base-objects",
        "--no-warnings",
        "--no-watch",
        "--no-watch-preserve-output",
        "--no-zero-fill-buffers",
        "--node-memory-debug",
        "--node-snapshot",
        "--openssl-config",
        "--openssl-legacy-provider",
        "--openssl-shared-config",
        "--pending-deprecation",
        "--perf-basic-prof",
        "--perf-basic-prof-only-functions",
        "--perf-prof",
        "--perf-prof-unwinding-info",
        "--permission",
        "--permission-audit",
        "--preserve-symlinks",
        "--preserve-symlinks-main",
        "--prof-process",
        "--redirect-warnings",
        "--report-compact",
        "--report-dir",
        "--report-directory",
        "--report-exclude-env",
        "--report-exclude-network",
        "--report-filename",
        "--report-on-fatalerror",
        "--report-on-signal",
        "--report-signal",
        "--report-uncaught-exception",
        "--require",
        "--require-module",
        "--secure-heap",
        "--secure-heap-min",
        "--snapshot-blob",
        "--stack-trace-limit",
        "--strip-types",
        "--test-coverage-branches",
        "--test-coverage-exclude",
        "--test-coverage-functions",
        "--test-coverage-include",
        "--test-coverage-lines",
        "--test-global-setup",
        "--test-isolation",
        "--test-name-pattern",
        "--test-only",
        "--test-reporter",
        "--test-reporter-destination",
        "--test-rerun-failures",
        "--test-shard",
        "--test-skip-pattern",
        "--throw-deprecation",
        "--title",
        "--tls-cipher-list",
        "--tls-keylog",
        "--tls-max-v1.2",
        "--tls-max-v1.3",
        "--tls-min-v1.0",
        "--tls-min-v1.1",
        "--tls-min-v1.2",
        "--tls-min-v1.3",
        "--trace-deprecation",
        "--trace-env",
        "--trace-env-js-stack",
        "--trace-env-native-stack",
        "--trace-event-categories",
        "--trace-event-file-pattern",
        "--trace-events-enabled",
        "--trace-exit",
        "--trace-promises",
        "--trace-require-module",
        "--trace-sigint",
        "--trace-sync-io",
        "--trace-tls",
        "--trace-uncaught",
        "--trace-warnings",
        "--track-heap-objects",
        "--unhandled-rejections",
        "--use-bundled-ca",
        "--use-env-proxy",
        "--use-largepages",
        "--use-openssl-ca",
        "--use-system-ca",
        "--v8-pool-size",
        "--verify-base-objects",
        "--warnings",
        "--watch",
        "--watch-kill-signal",
        "--watch-path",
        "--watch-preserve-output",
        "--webstorage",
        "--zero-fill-buffers",
        "-C",
        "-r",
    ];
    module_set_value(FLAGS)
}
