//! Child Process module - provides process spawning capabilities

// #1934: live-streaming `spawn` reactor (non-blocking child, stdout/stderr
// pumped through the event loop, live `stdin.write()` / `kill()`).
pub mod reactor;
// #1933: `fork()` + IPC channel (parent `send`/`'message'`/`disconnect`, child
// `process.send`/`process.on('message')`).
pub mod fork;
// #2130: V8 structured-clone codec for `serialization: 'advanced'` IPC.
mod v8_serde;
// #2555: sync buffered `input`, `timeout`, and `maxBuffer` execution options.
mod sync_run;
// #3079: setup-time command/file/args validation (`ERR_INVALID_ARG_TYPE`).
mod validate;
pub use validate::{js_child_process_validate_args, js_child_process_validate_command};

// #3137: reuse the codec for the public `node:v8` serialize/deserialize API.
// #3680: class-based `v8.Serializer` / `v8.Deserializer` builders.
pub(crate) use v8_serde::{
    v8_class_deserializer_new, v8_class_deserializer_read_double,
    v8_class_deserializer_read_header, v8_class_deserializer_read_raw_bytes,
    v8_class_deserializer_read_uint32, v8_class_deserializer_read_uint64,
    v8_class_deserializer_read_value, v8_class_serializer_new, v8_class_serializer_release,
    v8_class_serializer_write_double, v8_class_serializer_write_header,
    v8_class_serializer_write_raw_bytes, v8_class_serializer_write_uint32,
    v8_class_serializer_write_uint64, v8_class_serializer_write_value, v8_deserialize,
    v8_serialize,
};

use std::process::{Command, Stdio};

#[cfg(test)]
use crate::object::js_object_get_field_by_name_f64;

use sync_run::{CpRun, CpRunError, CpRunOptions};

use crate::closure::{
    js_closure_alloc, js_closure_get_capture_ptr, js_closure_set_capture_ptr, ClosureHeader,
};
use crate::object::{js_object_set_field_by_name, ObjectHeader};
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::JSValue;

// ----------------------------------------------------------------------------
// Topical sub-modules (split out of this file; pure code move).
// ----------------------------------------------------------------------------
mod builder;
mod emitter;
mod exec;
mod options;
mod output;
mod registry;
mod signals;
mod value_util;

// Re-export every moved item that is referenced from outside its sibling
// (the existing `reactor` / `fork` / `sync_run` / `v8_serde` modules reach
// these via `use super::*` or `use super::{...}`, and some are
// `crate::child_process::...` public/crate API). Visibility matches the
// item's own visibility.

// registry.rs — background-process registry + detach FFI.
pub(crate) use registry::make_two_field_object;
pub use registry::{
    js_child_process_get_process_status, js_child_process_kill_process,
    js_child_process_spawn_background, js_child_process_spawn_detached, spawn_detached_command,
};

// value_util.rs — NaN-box value helpers.
pub(crate) use value_util::{
    cp_args_from_value, cp_array_ptr, cp_box_ptr, cp_box_string, cp_box_string_bytes,
    cp_coerce_string, cp_get_field, cp_make_buffer, cp_object_ptr, cp_read_arg_strings,
    cp_read_string_header, cp_set_field, cp_this, cp_undefined, cp_value_to_bytes,
    cp_value_to_string,
};

// signals.rs — signal name/number mapping + kill/timeout reads.
pub(crate) use signals::{
    cp_read_kill_signal, cp_read_timeout, cp_signal_from_value, cp_signal_name, CP_SIGTERM,
};

// emitter.rs — EventEmitter listener registry, method bodies, IPC send/disconnect.
pub(crate) use emitter::{
    cp_emit, cp_method_disconnect, cp_method_dispose, cp_method_emit, cp_method_kill, cp_method_on,
    cp_method_pipe, cp_method_read, cp_method_remove_all_listeners, cp_method_remove_listener,
    cp_method_send, cp_method_stdin_end, cp_method_this0, cp_method_this1, cp_method_write2,
    cp_send_callback_thunk, js_fork_child,
};

// builder.rs — heap object construction + shape ids.
pub(crate) use builder::{
    cp_build_object, cp_build_readable, cp_build_writable, cp_cast0, cp_cast1, cp_cast2, cp_cast4,
    cp_install_dispose, cp_register_arities, CpFn, CP_SHAPE_ID,
};

// options.rs — command option application (cwd/env/uid/gid/argv0/detached/stdio).
pub(crate) use options::{
    cp_abort_signal_is_aborted, cp_apply_argv0, cp_apply_detached, cp_apply_live_stdio,
    cp_apply_options, cp_build_command, cp_read_abort_signal, cp_read_stdio, cp_spawnargs_argv0,
    cp_stdio_from_fd, cp_stdio_js_value, CpStdio,
};

// output.rs — output encoding, error shape, exit decoding.
pub(crate) use output::{
    cp_abort_error, cp_box_output, cp_box_run_output, cp_decode_status, cp_errno_number,
    cp_exec_callback_args, cp_exec_callback_output_bytes, cp_file_cmd_display, cp_io_error_code,
    cp_make_error, cp_output_array, cp_read_output_mode, cp_sync_throw_error, CpExit, CpOutput,
};

// exec.rs — exec / execFile / spawnSync / execSync FFI + promisify wrappers.
pub(crate) use exec::make_promisified_child_process;
pub use exec::{
    js_child_process_exec, js_child_process_exec_file, js_child_process_exec_file_sync,
    js_child_process_exec_sync, js_child_process_spawn, js_child_process_spawn_sync,
};

// ============================================================================
// NaN-boxing tag constants (inline to avoid pub(crate) visibility issues)
// ============================================================================
pub(crate) const TAG_NULL_BITS: u64 = 0x7FFC_0000_0000_0002;
pub(crate) const TAG_UNDEFINED_BITS: u64 = 0x7FFC_0000_0000_0001;
pub(crate) const TAG_TRUE_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0004u64);
pub(crate) const TAG_FALSE_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0003u64);
pub(crate) const TAG_NULL_F64: f64 = f64::from_bits(0x7FFC_0000_0000_0002u64);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exec_sync_echo() {
        let cmd = "echo hello";
        let cmd_ptr = js_string_from_bytes(cmd.as_ptr(), cmd.len() as u32);
        let result = js_child_process_exec_sync(cmd_ptr, std::ptr::null());

        // #1937: execSync returns a Buffer by default; verify it carries the
        // echoed bytes.
        assert!(JSValue::from_bits(result.to_bits()).is_pointer());
        let buf =
            (result.to_bits() & crate::value::POINTER_MASK) as *const crate::buffer::BufferHeader;
        assert!(!buf.is_null());
        unsafe {
            assert!((*buf).length > 0);
        }
    }

    #[test]
    fn test_spawn_sync_result_fields() {
        // #1936: spawnSync result carries pid / output / stdout / stderr /
        // status / signal.
        let cmd = "echo";
        let cmd_ptr = js_string_from_bytes(cmd.as_ptr(), cmd.len() as u32);
        let args = crate::array::js_array_alloc(1);
        let hi = js_string_from_bytes(b"hi".as_ptr(), 2);
        crate::array::js_array_push_f64(args, crate::value::js_nanbox_string(hi as i64));

        let result = js_child_process_spawn_sync(cmd_ptr, args, std::ptr::null());
        assert!(!result.is_null());
        let get = |name: &[u8]| -> f64 {
            let k = js_string_from_bytes(name.as_ptr(), name.len() as u32);
            js_object_get_field_by_name_f64(result, k)
        };
        // status should be the numeric exit code 0.
        assert_eq!(get(b"status"), 0.0);
        // pid is a positive number.
        assert!(get(b"pid") > 0.0);
        // output / stdout / stderr are present (pointers, not undefined).
        for f in [
            b"output".as_slice(),
            b"stdout".as_slice(),
            b"stderr".as_slice(),
        ] {
            assert!(JSValue::from_bits(get(f).to_bits()).is_pointer());
        }
        // signal is null on a clean exit.
        assert_eq!(get(b"signal").to_bits(), TAG_NULL_BITS);
    }
}
