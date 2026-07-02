//! Built-in functions and objects.
//!
//! Provides runtime implementations of JavaScript built-ins like console.log,
//! `util.format`, `parseInt`, `Number(...)` coercion, etc. Originally a single
//! ~4,100 line file; split into topical submodules so no individual file
//! exceeds the 2,000-line ceiling.
//!
//! Re-exports below preserve the historical `crate::builtins::js_*` import
//! paths used by sibling runtime modules (`gc`, `object`, `promise`,
//! `proxy`, `value`, …) and by the rest of the crate's externally-visible
//! `#[no_mangle] pub extern "C"` FFI surface consumed by perry-emitted LLVM.

// On harmonyos, the .so is loaded by ArkTS and stdout/stderr have no
// terminal — every `println!("foo")` from inside Perry's runtime
// disappears into the void. Override `println!` to route through hilog
// (libhilog_ndk.z.so::OH_LOG_Print) instead, so `console.log("hi")`
// surfaces in DevEco/hdc the same way ArkTS's console.log does. The
// shadowing is module-scoped (only this builtins tree) so other runtime
// modules keep their normal stdout-bound println! semantics — only the
// user-facing console.* family routes to hilog.
#[cfg(feature = "ohos-napi")]
macro_rules! println {
    () => {
        $crate::arkts_callbacks::ohos_stdout_println("")
    };
    ($($arg:tt)*) => {
        $crate::arkts_callbacks::ohos_stdout_println(&format!($($arg)*))
    };
}

// Make the override visible to the topical submodules below via `use
// super::println;` (each submodule that prints needs the same shadow so
// stdout output on harmonyos routes through hilog the same way it did
// pre-split).
#[cfg(feature = "ohos-napi")]
pub(crate) use println;

pub(crate) use crate::string::{js_string_from_bytes, js_string_from_wtf8_bytes, StringHeader};
pub(crate) use crate::JSValue;

mod arithmetic;
mod console;
mod formatting;
mod globals;
mod numbers;
mod table;

// Explicit named re-exports (NOT globs — globs don't propagate transitively
// through `pub use` chains, and external callers reach in via the historical
// `crate::builtins::js_*` path).

pub use arithmetic::{
    js_add, js_div, js_eq, js_ge, js_gt, js_le, js_loose_eq, js_lt, js_mod, js_mul, js_rel_ge,
    js_rel_gt, js_rel_le, js_rel_lt, js_sub, js_value_typeof,
};

pub use console::{
    js_console_assert, js_console_assert_spread, js_console_clear, js_console_context,
    js_console_count, js_console_count_reset, js_console_count_reset_value, js_console_count_value,
    js_console_create_task, js_console_dir_with_options, js_console_error_dynamic,
    js_console_error_i32, js_console_error_number, js_console_error_spread, js_console_group,
    js_console_group_begin, js_console_group_end, js_console_log, js_console_log_as_closure,
    js_console_log_dynamic, js_console_log_i32, js_console_log_i64, js_console_log_number,
    js_console_log_spread, js_console_method_by_value, js_console_new, js_console_new2,
    js_console_noop, js_console_time, js_console_time_end, js_console_time_end_value,
    js_console_time_log, js_console_time_log_spread, js_console_time_log_value,
    js_console_time_value, js_console_trace, js_console_trace_spread, js_console_warn_dynamic,
    js_console_warn_i32, js_console_warn_number, js_console_warn_spread, perry_debug_trace_init,
    perry_debug_trace_init_done, scan_console_log_singleton_roots,
    scan_console_log_singleton_roots_mut,
};

pub(crate) use console::{
    is_console_instance_method_name, is_console_instance_value,
    try_console_instance_method_dispatch, CONSOLE_INSTANCE_CLASS_ID,
};

pub use formatting::{
    function_name_for_ptr, function_source_for_func_ptr, function_source_for_ptr, js_array_print,
    js_boxed_bigint_new, js_boxed_boolean_new, js_boxed_number_new, js_boxed_string_new,
    js_boxed_symbol_new, js_register_function_name, js_register_function_source, js_util_format,
    js_util_format_with_options, js_util_inspect, js_util_is_deep_strict_equal,
    js_util_is_deep_strict_equal_skip_prototype, js_util_strip_vt_control_characters,
    register_function_name_if_absent, scan_boxed_primitive_payload_roots_mut,
};

pub(crate) use formatting::{
    boxed_primitive_json_value, boxed_primitive_payload, boxed_primitive_to_string_tag,
    format_finite_number_js, format_jsvalue, is_negative_zero, jsvalue_string_content,
    InspectCompactGuard, InspectCustomInspectGuard, InspectDepthLimitGuard, InspectGettersGuard,
    InspectShowHiddenGuard, InspectSortedGuard,
};

pub use globals::{
    js_decode_uri, js_decode_uri_component, js_drain_queued_microtasks, js_encode_uri,
    js_encode_uri_component, js_escape, js_queue_microtask, js_queue_next_tick,
    js_queue_next_tick_args, js_structured_clone, js_text_decoder_decode, js_text_encoder_encode,
    js_unescape, restore_queued_microtask_contexts, scan_queued_microtask_roots,
    scan_queued_microtask_roots_mut,
};

pub(crate) use globals::{drain_queued_microtasks_count, queued_microtasks_pending};

pub use numbers::{
    js_is_finite, js_is_nan, js_number_coerce, js_number_is_finite, js_number_is_integer,
    js_number_is_nan, js_number_is_safe_integer, js_parse_float, js_parse_int, js_string_coerce,
    js_to_integer_or_infinity, reject_symbol_to_string,
};

#[cfg(test)]
pub(crate) use numbers::parse_float_bytes;

pub use table::{js_console_table, js_console_table_with_properties};

#[cfg(test)]
pub(crate) use console::{test_console_log_singleton, test_set_console_log_singleton};

#[cfg(test)]
pub(crate) use globals::{
    test_queued_microtask_snapshot, test_seed_queued_microtask,
    test_seed_queued_microtask_previous_context,
};
