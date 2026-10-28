//! `process.report` diagnostic-report builders plus the related
//! `process.release` / `process.features` / `process.config` /
//! `process.allowedNodeEnvironmentFlags` value constructors. Split out of the
//! `process` trunk. Pure code move — no behavior change.

use super::*;
use crate::value::JSValue;

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

pub(crate) use super::metadata_values::{
    process_allowed_flags_value, process_config_value, process_features_value,
    process_release_value,
};
