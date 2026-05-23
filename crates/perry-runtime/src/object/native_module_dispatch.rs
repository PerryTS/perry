//! Dispatch table for `(native_module_namespace).method(...)` calls
//! that escape into the runtime tower from
//! `js_native_call_method`.
//!
//! Split out of `object/mod.rs` (issue #1103). Pure relocation — no
//! logic changes.

use super::*;

/// Dispatch a method call on a native module namespace object.
/// Extracts the module name from the object and dispatches to the appropriate
/// runtime function based on (module_name, method_name).
pub(crate) unsafe fn dispatch_native_module_method(
    obj: *const ObjectHeader,
    method_name: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> f64 {
    // Extract the module name from field 0 of the namespace object
    let module_field = js_object_get_field(obj as *mut _, 0);
    let module_name = if module_field.is_string() {
        let str_ptr = module_field.as_string_ptr();
        let len = (*str_ptr).byte_len as usize;
        let data = (str_ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        std::str::from_utf8(std::slice::from_raw_parts(data, len)).unwrap_or("")
    } else {
        ""
    };
    // Helper: get arg N as f64
    let arg = |n: usize| -> f64 {
        if n < args_len && !args_ptr.is_null() {
            *args_ptr.add(n)
        } else {
            f64::from_bits(JSValue::undefined().bits())
        }
    };

    // Helper: extract raw string pointer from a NaN-boxed f64 value
    let arg_str_ptr = |n: usize| -> *const crate::StringHeader {
        let v = arg(n);
        let jsv = JSValue::from_bits(v.to_bits());
        if jsv.is_string() {
            jsv.as_string_ptr()
        } else {
            std::ptr::null()
        }
    };

    // Helper: convert i32 boolean to NaN-boxed TAG_TRUE / TAG_FALSE
    let bool_to_f64 = |v: i32| -> f64 {
        if v != 0 {
            f64::from_bits(0x7FFC_0000_0000_0004) // TAG_TRUE
        } else {
            f64::from_bits(0x7FFC_0000_0000_0003) // TAG_FALSE
        }
    };

    // Helper: convert *mut StringHeader to NaN-boxed string f64
    let str_to_f64 =
        |ptr: *mut crate::StringHeader| -> f64 { f64::from_bits(JSValue::string_ptr(ptr).bits()) };
    let pack_args = || -> *mut crate::array::ArrayHeader {
        let mut arr = crate::array::js_array_alloc(args_len as u32);
        for i in 0..args_len {
            arr = crate::array::js_array_push_f64(arr, arg(i));
        }
        arr
    };
    let pack_args_from = |start: usize| -> *mut crate::array::ArrayHeader {
        let len = args_len.saturating_sub(start);
        let mut arr = crate::array::js_array_alloc(len as u32);
        for i in start..args_len {
            arr = crate::array::js_array_push_f64(arr, arg(i));
        }
        arr
    };
    let label_arg_ptr = |n: usize| -> *const crate::StringHeader {
        if n >= args_len || args_ptr.is_null() {
            return std::ptr::null();
        }
        let v = arg(n);
        if JSValue::from_bits(v.to_bits()).is_undefined() {
            std::ptr::null()
        } else {
            crate::builtins::js_string_coerce(v) as *const crate::StringHeader
        }
    };
    let bool_tag = |v: bool| -> f64 {
        if v {
            f64::from_bits(0x7FFC_0000_0000_0004)
        } else {
            f64::from_bits(0x7FFC_0000_0000_0003)
        }
    };
    let ptr_addr = |v: f64| -> usize {
        let bits = v.to_bits();
        if (bits >> 48) >= 0x7FF8 {
            (bits & 0x0000_FFFF_FFFF_FFFF) as usize
        } else {
            bits as usize
        }
    };
    let typed_kind = |v: f64| -> Option<u8> {
        let addr = ptr_addr(v);
        if crate::buffer::is_uint8array_buffer(addr) {
            Some(crate::typedarray::KIND_UINT8)
        } else {
            crate::typedarray::lookup_typed_array_kind(addr)
        }
    };
    let usize_arg = |v: f64| -> Option<usize> {
        let raw = v.to_bits();
        let top16 = (raw >> 48) as u16;
        if JSValue::from_bits(raw).is_undefined() || JSValue::from_bits(raw).is_null() {
            return None;
        }
        if top16 == 0x7FFE {
            let i = (raw & 0xFFFF_FFFF) as u32 as i32;
            return (i >= 0).then_some(i as usize);
        }
        // `is_sign_negative` is true for -0.0 too (a normal result of
        // `0 - 0` in JS land). Accept -0.0 as 0 and only reject genuinely
        // negative numbers.
        if v.is_nan() || v.is_infinite() || (v.is_sign_negative() && v != 0.0) {
            None
        } else {
            Some(v as usize)
        }
    };

    match (module_name, method_name) {
        // ── crypto module ──
        ("crypto", "randomFillSync") if args_len >= 1 => {
            use rand::RngCore;
            let target = arg(0);
            let addr = ptr_addr(target);
            let offset = usize_arg(arg(1));
            let size = usize_arg(arg(2));
            let range = |total: usize| -> (usize, usize) {
                let start = offset.unwrap_or(0).min(total);
                let end = size
                    .map(|s| start.saturating_add(s).min(total))
                    .unwrap_or(total);
                (start, end)
            };
            if crate::typedarray::lookup_typed_array_kind(addr).is_some() {
                let ta = addr as *mut crate::typedarray::TypedArrayHeader;
                let len = (*ta).length as usize;
                let elem_size = (*ta).elem_size as usize;
                let (start_elem, end_elem) = range(len);
                let start = start_elem.saturating_mul(elem_size);
                let end = end_elem.saturating_mul(elem_size);
                if end > start {
                    let data = (addr as *mut u8)
                        .add(std::mem::size_of::<crate::typedarray::TypedArrayHeader>());
                    rand::thread_rng()
                        .fill_bytes(std::slice::from_raw_parts_mut(data.add(start), end - start));
                }
                target
            } else if crate::buffer::is_registered_buffer(addr) {
                let buf = addr as *mut crate::buffer::BufferHeader;
                let total = (*buf).length as usize;
                let (start, end) = range(total);
                if end > start {
                    let data = crate::buffer::buffer_data_mut(buf);
                    rand::thread_rng()
                        .fill_bytes(std::slice::from_raw_parts_mut(data.add(start), end - start));
                }
                target
            } else {
                target
            }
        }

        // ── tty module ──
        ("tty", "isatty") => crate::tty::js_tty_isatty(arg(0)),

        // ── perf_hooks module (performance.*) ──
        // Statically lowered at call sites (module_static.rs); these arms
        // also serve the generic namespace-object method-dispatch path.
        ("perf_hooks", "now") => crate::date::js_performance_now(),
        ("perf_hooks", "mark") => crate::perf_hooks::js_perf_mark(arg(0), arg(1)),
        ("perf_hooks", "measure") => crate::perf_hooks::js_perf_measure(arg(0), arg(1), arg(2)),
        ("perf_hooks", "getEntries") => crate::perf_hooks::js_perf_get_entries(),
        ("perf_hooks", "getEntriesByType") => {
            crate::perf_hooks::js_perf_get_entries_by_type(arg(0))
        }
        ("perf_hooks", "getEntriesByName") => {
            crate::perf_hooks::js_perf_get_entries_by_name(arg(0), arg(1))
        }
        ("perf_hooks", "clearMarks") => crate::perf_hooks::js_perf_clear_marks(arg(0)),
        ("perf_hooks", "clearMeasures") => crate::perf_hooks::js_perf_clear_measures(arg(0)),
        ("perf_hooks", "eventLoopUtilization") => {
            crate::perf_hooks::js_perf_event_loop_utilization(arg(0))
        }
        ("perf_hooks", "toJSON") => crate::perf_hooks::js_perf_to_json(),
        ("perf_hooks", "clearResourceTimings") => {
            crate::perf_hooks::js_perf_clear_resource_timings()
        }
        ("perf_hooks", "setResourceTimingBufferSize") => {
            crate::perf_hooks::js_perf_set_resource_timing_buffer_size(arg(0))
        }

        // ── PerformanceObserver instance (perf_observer) ──
        // The registry index lives in field[1] of the namespace object; the
        // runtime fns re-derive it from the object value.
        ("perf_observer", "observe") => {
            let obs_val = crate::value::js_nanbox_pointer(obj as i64);
            crate::perf_hooks::js_perf_observer_observe(obs_val, arg(0))
        }
        ("perf_observer", "disconnect") => {
            let obs_val = crate::value::js_nanbox_pointer(obj as i64);
            crate::perf_hooks::js_perf_observer_disconnect(obs_val)
        }
        ("perf_observer", "takeRecords") => {
            let obs_val = crate::value::js_nanbox_pointer(obj as i64);
            crate::perf_hooks::js_perf_observer_take_records(obs_val)
        }

        // ── PerformanceObserverEntryList (the callback `list` arg) ──
        ("perf_observer_list", "getEntries") => crate::perf_hooks::current_list_get_entries(),
        ("perf_observer_list", "getEntriesByType") => {
            crate::perf_hooks::current_list_get_by_type(arg(0))
        }
        ("perf_observer_list", "getEntriesByName") => {
            crate::perf_hooks::current_list_get_by_name(arg(0))
        }

        // ── timers module ──
        ("timers", "setTimeout") if args_len >= 2 => {
            let cb = arg(0);
            let delay = arg(1);
            let cb_handle = {
                let bits = cb.to_bits();
                if (bits >> 48) >= 0x7FF8 {
                    (bits & 0x0000_FFFF_FFFF_FFFF) as i64
                } else {
                    bits as i64
                }
            };
            if args_len > 2 {
                let extra_ptr = unsafe { args_ptr.add(2) };
                return f64::from_bits(
                    JSValue::pointer(crate::timer::js_set_timeout_callback_args(
                        cb_handle,
                        delay,
                        extra_ptr,
                        (args_len - 2) as i32,
                    ) as *mut u8)
                    .bits(),
                );
            }
            return f64::from_bits(JSValue::pointer(
                crate::timer::js_set_timeout_callback(cb_handle, delay) as *mut u8,
            ).bits());
        }
        ("timers", "setImmediate") if args_len >= 1 => {
            let cb = arg(0);
            let cb_handle = {
                let bits = cb.to_bits();
                if (bits >> 48) >= 0x7FF8 {
                    (bits & 0x0000_FFFF_FFFF_FFFF) as i64
                } else {
                    bits as i64
                }
            };
            if args_len > 1 {
                let extra_ptr = unsafe { args_ptr.add(1) };
                return f64::from_bits(
                    JSValue::pointer(crate::timer::js_set_immediate_callback_args(
                        cb_handle,
                        extra_ptr,
                        (args_len - 1) as i32,
                    ) as *mut u8)
                    .bits(),
                );
            }
            return f64::from_bits(
                JSValue::pointer(crate::timer::js_set_immediate_callback(cb_handle) as *mut u8)
                    .bits(),
            );
        }
        ("timers", "setInterval") if args_len >= 2 => {
            let cb = arg(0);
            let delay = arg(1);
            let bits = cb.to_bits();
            let cb_handle = if (bits >> 48) >= 0x7FF8 {
                (bits & 0x0000_FFFF_FFFF_FFFF) as i64
            } else {
                bits as i64
            };
            return f64::from_bits(
                JSValue::pointer(crate::timer::setInterval(cb_handle, delay) as *mut u8).bits(),
            );
        }
        ("timers", "clearTimeout") | ("timers", "clearImmediate") if args_len >= 1 => {
            let id_bits = arg(0).to_bits();
            let id = if (id_bits >> 48) >= 0x7FF8 {
                (id_bits & 0x0000_FFFF_FFFF_FFFF) as i64
            } else {
                id_bits as i64
            };
            crate::timer::clearTimeout(id);
            return f64::from_bits(JSValue::undefined().bits());
        }
        ("timers", "clearInterval") if args_len >= 1 => {
            let id_bits = arg(0).to_bits();
            let id = if (id_bits >> 48) >= 0x7FF8 {
                (id_bits & 0x0000_FFFF_FFFF_FFFF) as i64
            } else {
                id_bits as i64
            };
            crate::timer::clearInterval(id);
            return f64::from_bits(JSValue::undefined().bits());
        }
        // ── assert module ──
        // Root-callable `assert(x, msg)` / `assert.strict(x, msg)` —
        // HIR lowers these to method "default".
        ("assert", "default") | ("assert/strict", "default") => js_assert_ok(arg(0), arg(1)),
        ("assert", "strict") => js_assert_ok(arg(0), arg(1)),
        ("assert", "ok") | ("assert/strict", "ok") => js_assert_ok(arg(0), arg(1)),
        ("assert", "fail") | ("assert/strict", "fail") => js_assert_fail(arg(0)),
        ("assert", "equal") => js_assert_equal(arg(0), arg(1), arg(2)),
        ("assert", "notEqual") => js_assert_not_equal(arg(0), arg(1), arg(2)),
        ("assert", "strictEqual")
        | ("assert/strict", "strictEqual")
        | ("assert/strict", "equal") => js_assert_strict_equal(arg(0), arg(1), arg(2)),
        ("assert", "notStrictEqual")
        | ("assert/strict", "notStrictEqual")
        | ("assert/strict", "notEqual") => js_assert_not_strict_equal(arg(0), arg(1), arg(2)),
        ("assert", "deepEqual") => js_assert_deep_equal(arg(0), arg(1), arg(2)),
        ("assert", "notDeepEqual") => js_assert_not_deep_equal(arg(0), arg(1), arg(2)),
        ("assert", "deepStrictEqual")
        | ("assert/strict", "deepStrictEqual")
        | ("assert/strict", "deepEqual") => js_assert_deep_strict_equal(arg(0), arg(1), arg(2)),
        ("assert", "notDeepStrictEqual")
        | ("assert/strict", "notDeepStrictEqual")
        | ("assert/strict", "notDeepEqual") => {
            js_assert_not_deep_strict_equal(arg(0), arg(1), arg(2))
        }
        ("assert", "match") | ("assert/strict", "match") => js_assert_match(arg(0), arg(1), arg(2)),
        ("assert", "doesNotMatch") | ("assert/strict", "doesNotMatch") => {
            js_assert_does_not_match(arg(0), arg(1), arg(2))
        }
        ("assert", "ifError") | ("assert/strict", "ifError") => js_assert_if_error(arg(0)),

        // ── fs module (args are NaN-boxed f64, booleans return as i32→f64) ──
        ("fs", "existsSync") => bool_to_f64(crate::fs::js_fs_exists_sync(arg(0))),
        ("fs", "readFileSync") => crate::fs::js_fs_read_file_dispatch(arg(0), arg(1)),
        ("fs", "writeFileSync") => bool_to_f64(crate::fs::js_fs_write_file_sync_options(
            arg(0),
            arg(1),
            arg(2),
        )),
        ("fs", "appendFileSync") => bool_to_f64(crate::fs::js_fs_append_file_sync_options(
            arg(0),
            arg(1),
            arg(2),
        )),
        ("fs", "mkdirSync") => bool_to_f64(crate::fs::js_fs_mkdir_sync_options(arg(0), arg(1))),
        ("fs", "unlinkSync") => bool_to_f64(crate::fs::js_fs_unlink_sync(arg(0))),
        ("fs", "rmSync") => bool_to_f64(crate::fs::js_fs_rm_recursive_options(arg(0), arg(1))),
        ("fs", "rmdirSync") => bool_to_f64(crate::fs::js_fs_rmdir_sync_options(arg(0), arg(1))),
        ("fs", "readdirSync") => {
            let raw = crate::fs::js_fs_readdir_sync(arg(0), arg(1));
            f64::from_bits(JSValue::pointer(raw.to_bits() as *const u8).bits())
        }
        ("fs", "statSync") => crate::fs::js_fs_stat_sync_options(arg(0), arg(1)),
        ("fs", "lstatSync") => crate::fs::js_fs_lstat_sync_options(arg(0), arg(1)),
        ("fs", "renameSync") => bool_to_f64(crate::fs::js_fs_rename_sync(arg(0), arg(1))),
        ("fs", "copyFileSync") => bool_to_f64(crate::fs::js_fs_copy_file_sync_flags(
            arg(0),
            arg(1),
            arg(2),
        )),
        ("fs", "cpSync") => bool_to_f64(crate::fs::js_fs_cp_sync_options(arg(0), arg(1), arg(2))),
        ("fs", "accessSync") => bool_to_f64(crate::fs::js_fs_access_sync_mode(arg(0), arg(1))),
        ("fs", "realpathSync") => crate::fs::js_fs_realpath_dispatch(arg(0), arg(1)),
        ("fs", "mkdtempSync") => crate::fs::js_fs_mkdtemp_dispatch(arg(0), arg(1)),
        ("fs", "chmodSync") => bool_to_f64(crate::fs::js_fs_chmod_sync(arg(0), arg(1))),
        ("fs", "chownSync") => bool_to_f64(crate::fs::js_fs_chown_sync(arg(0), arg(1), arg(2))),
        ("fs", "lchownSync") => bool_to_f64(crate::fs::js_fs_lchown_sync(arg(0), arg(1), arg(2))),
        ("fs", "truncateSync") => bool_to_f64(crate::fs::js_fs_truncate_sync(arg(0), arg(1))),
        ("fs", "ftruncateSync") => bool_to_f64(crate::fs::js_fs_ftruncate_sync(arg(0), arg(1))),
        ("fs", "fsyncSync") => bool_to_f64(crate::fs::js_fs_fsync_sync(arg(0))),
        ("fs", "fdatasyncSync") => bool_to_f64(crate::fs::js_fs_fdatasync_sync(arg(0))),
        ("fs", "fchmodSync") => bool_to_f64(crate::fs::js_fs_fchmod_sync(arg(0), arg(1))),
        ("fs", "fchownSync") => bool_to_f64(crate::fs::js_fs_fchown_sync(arg(0), arg(1), arg(2))),
        ("fs", "fstatSync") => crate::fs::js_fs_fstat_sync_options(arg(0), arg(1)),
        ("fs", "utimesSync") => crate::fs::js_fs_utimes_sync(arg(0), arg(1), arg(2)) as f64,
        ("fs", "lutimesSync") => crate::fs::js_fs_lutimes_sync(arg(0), arg(1), arg(2)) as f64,
        ("fs", "futimesSync") => crate::fs::js_fs_futimes_sync(arg(0), arg(1), arg(2)) as f64,
        ("fs", "readvSync") => crate::fs::js_fs_readv_sync(arg(0), arg(1), arg(2)),
        ("fs", "writevSync") => crate::fs::js_fs_writev_sync(arg(0), arg(1), arg(2)),
        ("fs", "statfsSync") => crate::fs::js_fs_statfs_sync_options(arg(0), arg(1)),
        ("fs", "opendirSync") => crate::fs::js_fs_opendir_sync(arg(0)),
        ("fs", "globSync") => {
            let raw = crate::fs::js_fs_glob_sync_options(arg(0), arg(1));
            f64::from_bits(JSValue::pointer(raw.to_bits() as *const u8).bits())
        }
        ("fs", "watch") => crate::fs::js_fs_watch(arg(0), arg(1), arg(2)),
        ("fs", "watchFile") => crate::fs::js_fs_watch_file(arg(0), arg(1), arg(2)),
        ("fs", "unwatchFile") => crate::fs::js_fs_unwatch_file(arg(0), arg(1)),
        ("fs", "linkSync") => bool_to_f64(crate::fs::js_fs_link_sync(arg(0), arg(1))),
        ("fs", "symlinkSync") => bool_to_f64(crate::fs::js_fs_symlink_sync(arg(0), arg(1))),
        ("fs", "readlinkSync") => crate::fs::js_fs_readlink_dispatch(arg(0), arg(1)),
        ("fs", "openSync") => crate::fs::js_fs_open_sync(arg(0), arg(1)),
        ("fs", "closeSync") => bool_to_f64(crate::fs::js_fs_close_sync(arg(0))),
        ("fs", "readSync") if args_len == 3 => {
            crate::fs::js_fs_read_sync_options(arg(0), arg(1), arg(2))
        }
        ("fs", "readSync") => crate::fs::js_fs_read_sync(arg(0), arg(1), arg(2), arg(3), arg(4)),
        ("fs", "writeSync") if args_len >= 5 => {
            crate::fs::js_fs_write_buffer_sync(arg(0), arg(1), arg(2), arg(3), arg(4))
        }
        ("fs", "writeSync") if args_len >= 3 => {
            crate::fs::js_fs_write_sync_options_dispatch(arg(0), arg(1), arg(2))
        }
        ("fs", "writeSync") => crate::fs::js_fs_write_sync(arg(0), arg(1)),
        ("fs", "read") if args_len == 4 => {
            crate::fs::js_fs_read_callback_options(arg(0), arg(1), arg(2), arg(3))
        }
        ("fs", "read") => {
            crate::fs::js_fs_read_callback(arg(0), arg(1), arg(2), arg(3), arg(4), arg(5))
        }
        ("fs", "write") if args_len >= 6 => {
            crate::fs::js_fs_write_buffer_callback(arg(0), arg(1), arg(2), arg(3), arg(4), arg(5))
        }
        ("fs", "write") if args_len == 4 => {
            crate::fs::js_fs_write_buffer_callback_options(arg(0), arg(1), arg(2), arg(3))
        }
        ("fs", "write") => crate::fs::js_fs_write_callback(arg(0), arg(1), arg(2)),
        ("fs", "readv") => crate::fs::js_fs_readv_callback(arg(0), arg(1), arg(2), arg(3)),
        ("fs", "writev") => crate::fs::js_fs_writev_callback(arg(0), arg(1), arg(2), arg(3)),
        ("fs", "createWriteStream") => crate::fs::js_fs_create_write_stream(arg(0), arg(1)),
        ("fs", "createReadStream") => crate::fs::js_fs_create_read_stream(arg(0), arg(1)),
        ("fs", "readFile") => crate::fs::js_fs_read_file_callback(arg(0), arg(1), arg(2)),
        ("fs", "writeFile") => crate::fs::js_fs_write_file_callback(arg(0), arg(1), arg(2), arg(3)),
        ("fs", "appendFile") => {
            crate::fs::js_fs_append_file_callback(arg(0), arg(1), arg(2), arg(3))
        }
        ("fs", "chmod") => crate::fs::js_fs_chmod_callback(arg(0), arg(1), arg(2)),
        ("fs", "chown") => crate::fs::js_fs_chown_callback(arg(0), arg(1), arg(2), arg(3)),
        ("fs", "lchown") => crate::fs::js_fs_lchown_callback(arg(0), arg(1), arg(2), arg(3)),
        ("fs", "truncate") => crate::fs::js_fs_truncate_callback(arg(0), arg(1), arg(2)),
        ("fs", "link") => crate::fs::js_fs_link_callback(arg(0), arg(1), arg(2)),
        ("fs", "symlink") => crate::fs::js_fs_symlink_callback(arg(0), arg(1), arg(2), arg(3)),
        ("fs", "readlink") => crate::fs::js_fs_readlink_callback(arg(0), arg(1), arg(2)),
        ("fs", "realpath") => crate::fs::js_fs_realpath_callback(arg(0), arg(1), arg(2)),
        ("fs", "mkdtemp") => crate::fs::js_fs_mkdtemp_callback(arg(0), arg(1), arg(2)),
        ("fs", "open") => crate::fs::js_fs_open_callback(arg(0), arg(1), arg(2), arg(3)),
        ("fs", "close") => crate::fs::js_fs_close_callback(arg(0), arg(1)),
        ("fs", "cp") => crate::fs::js_fs_cp_callback(arg(0), arg(1), arg(2), arg(3)),
        ("fs", "mkdir") => crate::fs::js_fs_mkdir_callback(arg(0), arg(1), arg(2)),
        ("fs", "unlink") => crate::fs::js_fs_unlink_callback(arg(0), arg(1)),
        ("fs", "rmdir") => crate::fs::js_fs_rmdir_callback(arg(0), arg(1), arg(2)),
        ("fs", "rm") => crate::fs::js_fs_rm_callback(arg(0), arg(1), arg(2)),
        ("fs", "access") => crate::fs::js_fs_access_callback(arg(0), arg(1), arg(2)),
        ("fs", "exists") => crate::fs::js_fs_exists_callback(arg(0), arg(1)),
        ("fs", "readdir") => crate::fs::js_fs_readdir_callback(arg(0), arg(1), arg(2)),
        ("fs", "stat") => crate::fs::js_fs_stat_callback(arg(0), arg(1), arg(2)),
        ("fs", "lstat") => crate::fs::js_fs_lstat_callback(arg(0), arg(1), arg(2)),
        ("fs", "statfs") => crate::fs::js_fs_statfs_callback(arg(0), arg(1), arg(2)),
        ("fs", "opendir") => crate::fs::js_fs_opendir_callback(arg(0), arg(1), arg(2)),
        ("fs", "glob") => crate::fs::js_fs_glob_callback(arg(0), arg(1), arg(2)),
        ("fs", "fstat") => crate::fs::js_fs_fstat_callback(arg(0), arg(1), arg(2)),
        ("fs", "ftruncate") => crate::fs::js_fs_ftruncate_callback(arg(0), arg(1), arg(2)),
        ("fs", "fsync") => crate::fs::js_fs_fsync_callback(arg(0), arg(1)),
        ("fs", "fdatasync") => crate::fs::js_fs_fdatasync_callback(arg(0), arg(1)),
        ("fs", "fchmod") => crate::fs::js_fs_fchmod_callback(arg(0), arg(1), arg(2)),
        ("fs", "fchown") => crate::fs::js_fs_fchown_callback(arg(0), arg(1), arg(2), arg(3)),
        ("fs", "utimes") => crate::fs::js_fs_utimes_callback(arg(0), arg(1), arg(2), arg(3)),
        ("fs", "lutimes") => crate::fs::js_fs_lutimes_callback(arg(0), arg(1), arg(2), arg(3)),
        ("fs", "futimes") => crate::fs::js_fs_futimes_callback(arg(0), arg(1), arg(2), arg(3)),
        ("fs", "rename") => crate::fs::js_fs_rename_callback(arg(0), arg(1), arg(2)),
        ("fs", "copyFile") => crate::fs::js_fs_copy_file_callback(arg(0), arg(1), arg(2), arg(3)),
        ("fs", "isDirectory") => bool_to_f64(crate::fs::js_fs_is_directory(arg(0))),

        // ── os module (no args, return string or f64) ──
        ("os", "tmpdir") => str_to_f64(crate::os::js_os_tmpdir()),
        ("os", "homedir") => str_to_f64(crate::os::js_os_homedir()),
        ("os", "platform") => str_to_f64(crate::os::js_os_platform()),
        ("os", "arch") => str_to_f64(crate::os::js_os_arch()),
        ("os", "hostname") => str_to_f64(crate::os::js_os_hostname()),
        ("os", "type") => str_to_f64(crate::os::js_os_type()),
        ("os", "release") => str_to_f64(crate::os::js_os_release()),
        ("os", "eol") => str_to_f64(crate::os::js_os_eol()),
        ("os", "devNull") => str_to_f64(crate::os::js_os_dev_null()),
        ("os", "totalmem") => crate::os::js_os_totalmem(),
        ("os", "freemem") => crate::os::js_os_freemem(),
        ("os", "uptime") => crate::os::js_os_uptime(),
        ("os", "availableParallelism") => crate::os::js_os_available_parallelism(),
        ("os", "endianness") => str_to_f64(crate::os::js_os_endianness()),
        ("os", "machine") => str_to_f64(crate::os::js_os_machine()),
        ("os", "loadavg") => {
            f64::from_bits(JSValue::pointer(crate::os::js_os_loadavg() as *const u8).bits())
        }
        ("os", "version") => str_to_f64(crate::os::js_os_version()),
        ("os", "cpus") => {
            f64::from_bits(JSValue::pointer(crate::os::js_os_cpus() as *const u8).bits())
        }
        ("os", "networkInterfaces") => f64::from_bits(
            JSValue::pointer(crate::os::js_os_network_interfaces() as *const u8).bits(),
        ),
        ("os", "userInfo") => {
            f64::from_bits(JSValue::pointer(crate::os::js_os_user_info() as *const u8).bits())
        }

        // ── path module (args are NaN-boxed strings → extract raw StringHeader ptr) ──
        ("path", "dirname") => str_to_f64(crate::path::js_path_dirname(arg_str_ptr(0))),
        ("path", "basename") => str_to_f64(crate::path::js_path_basename(arg_str_ptr(0))),
        ("path", "extname") => str_to_f64(crate::path::js_path_extname(arg_str_ptr(0))),
        ("path", "resolve") => str_to_f64(crate::path::js_path_resolve(arg_str_ptr(0))),
        ("path", "join") => str_to_f64(crate::path::js_path_join(arg_str_ptr(0), arg_str_ptr(1))),
        ("path", "isAbsolute") => bool_to_f64(crate::path::js_path_is_absolute(arg_str_ptr(0))),

        // ── util module ──
        ("util", "format") => crate::builtins::js_util_format(pack_args()),
        ("util", "formatWithOptions") => {
            let effective = args_len.saturating_sub(1);
            let mut arr = crate::array::js_array_alloc(effective as u32);
            for i in 1..args_len {
                arr = crate::array::js_array_push_f64(arr, arg(i));
            }
            crate::builtins::js_util_format(arr)
        }
        ("util", "inspect") => crate::builtins::js_util_inspect(arg(0), arg(1)),
        ("util", "isDeepStrictEqual") => {
            crate::builtins::js_util_is_deep_strict_equal(arg(0), arg(1))
        }
        ("util", "stripVTControlCharacters") => {
            crate::builtins::js_util_strip_vt_control_characters(arg(0))
        }

        ("util", "isPromise") => {
            let v = JSValue::from_bits(arg(0).to_bits());
            bool_tag(
                v.is_pointer()
                    && crate::promise::js_is_promise(
                        v.as_pointer::<crate::promise::Promise>() as *mut crate::promise::Promise
                    ) != 0,
            )
        }
        ("util", "isArrayBuffer") | ("util", "isAnyArrayBuffer") => {
            bool_tag(crate::buffer::is_array_buffer(ptr_addr(arg(0))))
        }
        ("util", "isArrayBufferView") => {
            let addr = ptr_addr(arg(0));
            bool_tag(crate::buffer::is_uint8array_buffer(addr) || typed_kind(arg(0)).is_some())
        }
        ("util", "isTypedArray") => bool_tag(typed_kind(arg(0)).is_some()),
        ("util", "isUint8Array") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_UINT8))
        }
        ("util", "isUint16Array") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_UINT16))
        }
        ("util", "isInt32Array") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_INT32))
        }
        ("util", "isFloat64Array") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_FLOAT64))
        }
        ("util", "isMap") => bool_tag(crate::map::is_registered_map(ptr_addr(arg(0)))),
        ("util", "isSet") => bool_tag(crate::set::is_registered_set(ptr_addr(arg(0)))),

        // ── util.types namespace ──
        ("util.types", "isPromise") => {
            let v = JSValue::from_bits(arg(0).to_bits());
            bool_tag(
                v.is_pointer()
                    && crate::promise::js_is_promise(
                        v.as_pointer::<crate::promise::Promise>() as *mut crate::promise::Promise
                    ) != 0,
            )
        }
        ("util.types", "isArrayBuffer") | ("util.types", "isAnyArrayBuffer") => {
            bool_tag(crate::buffer::is_array_buffer(ptr_addr(arg(0))))
        }
        ("util.types", "isArrayBufferView") => {
            let addr = ptr_addr(arg(0));
            bool_tag(crate::buffer::is_uint8array_buffer(addr) || typed_kind(arg(0)).is_some())
        }
        ("util.types", "isTypedArray") => bool_tag(typed_kind(arg(0)).is_some()),
        ("util.types", "isUint8Array") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_UINT8))
        }
        ("util.types", "isUint16Array") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_UINT16))
        }
        ("util.types", "isInt32Array") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_INT32))
        }
        ("util.types", "isFloat64Array") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_FLOAT64))
        }
        ("util.types", "isMap") => bool_tag(crate::map::is_registered_map(ptr_addr(arg(0)))),
        ("util.types", "isSet") => bool_tag(crate::set::is_registered_set(ptr_addr(arg(0)))),
        ("util.types", "isDate") => {
            bool_tag(crate::date::is_registered_date_bits(arg(0).to_bits()))
        }
        ("util.types", "isRegExp") => {
            let v = JSValue::from_bits(arg(0).to_bits());
            bool_tag(v.is_pointer() && crate::regex::is_regex_pointer(v.as_pointer::<u8>()))
        }
        ("util.types", "isNumberObject") => crate::object::js_util_types_is_number_object(arg(0)),
        ("util.types", "isStringObject") => crate::object::js_util_types_is_string_object(arg(0)),
        ("util.types", "isBooleanObject") => crate::object::js_util_types_is_boolean_object(arg(0)),
        ("util.types", "isBoxedPrimitive") => {
            crate::object::js_util_types_is_boxed_primitive(arg(0))
        }

        // ── node:util/types direct module ──
        ("util/types", "isPromise") => {
            let v = JSValue::from_bits(arg(0).to_bits());
            bool_tag(
                v.is_pointer()
                    && crate::promise::js_is_promise(
                        v.as_pointer::<crate::promise::Promise>() as *mut crate::promise::Promise
                    ) != 0,
            )
        }
        ("util/types", "isArrayBuffer") | ("util/types", "isAnyArrayBuffer") => {
            bool_tag(crate::buffer::is_array_buffer(ptr_addr(arg(0))))
        }
        ("util/types", "isArrayBufferView") => {
            let addr = ptr_addr(arg(0));
            bool_tag(crate::buffer::is_uint8array_buffer(addr) || typed_kind(arg(0)).is_some())
        }
        ("util/types", "isTypedArray") => bool_tag(typed_kind(arg(0)).is_some()),
        ("util/types", "isUint8Array") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_UINT8))
        }
        ("util/types", "isUint16Array") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_UINT16))
        }
        ("util/types", "isInt32Array") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_INT32))
        }
        ("util/types", "isFloat64Array") => {
            bool_tag(typed_kind(arg(0)) == Some(crate::typedarray::KIND_FLOAT64))
        }
        ("util/types", "isMap") => bool_tag(crate::map::is_registered_map(ptr_addr(arg(0)))),
        ("util/types", "isSet") => bool_tag(crate::set::is_registered_set(ptr_addr(arg(0)))),
        ("util/types", "isDate") => {
            bool_tag(crate::date::is_registered_date_bits(arg(0).to_bits()))
        }
        ("util/types", "isRegExp") => {
            let v = JSValue::from_bits(arg(0).to_bits());
            bool_tag(v.is_pointer() && crate::regex::is_regex_pointer(v.as_pointer::<u8>()))
        }
        ("util/types", "isNumberObject") => crate::object::js_util_types_is_number_object(arg(0)),
        ("util/types", "isStringObject") => crate::object::js_util_types_is_string_object(arg(0)),
        ("util/types", "isBooleanObject") => crate::object::js_util_types_is_boolean_object(arg(0)),
        ("util/types", "isBoxedPrimitive") => {
            crate::object::js_util_types_is_boxed_primitive(arg(0))
        }
        // ── url module (module-level functions return NaN-boxed JS values) ──
        ("url", "fileURLToPath") => crate::url::js_url_file_url_to_path(arg(0)),
        ("url", "pathToFileURL") => crate::url::js_url_path_to_file_url(arg(0)),
        ("url", "domainToASCII") => crate::url::js_url_domain_to_ascii(arg(0)),
        ("url", "domainToUnicode") => crate::url::js_url_domain_to_unicode(arg(0)),
        ("url", "urlToHttpOptions") => crate::url::js_url_to_http_options(arg(0)),
        ("url", "format") => crate::url::js_url_format(arg(0), arg(1)),
        ("url", "parse") => crate::url::js_url_legacy_parse(arg(0), arg(1)),
        ("url", "resolve") => crate::url::js_url_legacy_resolve(arg(0), arg(1)),

        // ── console module namespace (`node:console` / `console`) ──
        ("console", "log") | ("console", "info") | ("console", "debug") | ("console", "dirxml") => {
            crate::builtins::js_console_log_spread(pack_args());
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "error") => {
            crate::builtins::js_console_error_spread(pack_args());
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "warn") => {
            crate::builtins::js_console_warn_spread(pack_args());
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "assert") => {
            crate::builtins::js_console_assert_spread(arg(0), pack_args_from(1) as i64);
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "dir") => {
            crate::builtins::js_console_log_dynamic(arg(0));
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "trace") => {
            crate::builtins::js_console_trace_spread(pack_args());
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "table") => {
            if args_len > 1 {
                crate::builtins::js_console_table_with_properties(arg(0), arg(1));
            } else {
                crate::builtins::js_console_table(arg(0));
            }
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "clear") => {
            crate::builtins::js_console_clear();
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "count") => {
            crate::builtins::js_console_count_value(arg(0));
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "countReset") => {
            crate::builtins::js_console_count_reset_value(arg(0));
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "time") => {
            crate::builtins::js_console_time_value(arg(0));
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "timeEnd") => {
            crate::builtins::js_console_time_end_value(arg(0));
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "timeLog") => {
            if args_len > 1 {
                crate::builtins::js_console_time_log_spread(arg(0), pack_args_from(1));
            } else {
                crate::builtins::js_console_time_log_value(arg(0));
            }
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "group") | ("console", "groupCollapsed") => {
            if args_len > 0 {
                crate::builtins::js_console_log_dynamic(arg(0));
            }
            crate::builtins::js_console_group_begin();
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "groupEnd") => {
            crate::builtins::js_console_group_end();
            f64::from_bits(JSValue::undefined().bits())
        }
        ("console", "profile") | ("console", "profileEnd") | ("console", "timeStamp") => {
            f64::from_bits(JSValue::undefined().bits())
        }

        _ => {
            // Method not found on native module — return undefined
            f64::from_bits(JSValue::undefined().bits())
        }
    }
}
