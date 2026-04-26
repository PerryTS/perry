//! Runtime function signature registry.
//!
//! These declare the FFI ABI for functions exported by `libperry_runtime.a`.
//! Phase 1 only needs a tiny subset — enough to print a number — so we start
//! with six entries. Each later phase adds what it needs; the goal is to
//! avoid declaring unused runtime symbols, which would force the linker to
//! pull in the whole runtime even for a trivial test.
//!
//! Signatures MUST match `perry-runtime/src/value.rs` and friends byte-for-byte.
//! Mismatch is silent and deadly — the generated code calls the function and
//! gets garbage back (see anvil README §48 bug hunt).

use crate::module::LlModule;
use crate::types::{DOUBLE, I32, I64, PTR, VOID};

/// Declare the minimum set of runtime functions needed by Phase 1
/// (`console.log(42)`):
/// - `js_console_log_dynamic(double)` — prints any NaN-boxed value
/// - `js_nanbox_string(i64) -> double` — wraps a raw string handle
/// - `js_nanbox_get_pointer(double) -> i64` — unwraps a NaN-boxed pointer
/// - `js_string_from_bytes(ptr, i32) -> i64` — interns a UTF-8 string
/// - `js_is_truthy(double) -> i32` — JS-ish truthiness test
/// - `js_gc_init()` — runtime bootstrap, called once at start of `main`
pub fn declare_phase1(module: &mut LlModule) {
    // GC / runtime bootstrap.
    module.declare_function("js_gc_init", VOID, &[]);
    // Handle-method dispatcher wiring (issue #86). Stdlib provides the
    // real impl; when only runtime is linked, it's a no-op stub.
    module.declare_function("js_stdlib_init_dispatch", VOID, &[]);

    // Console.
    module.declare_function("js_console_log_dynamic", VOID, &[DOUBLE]);
    module.declare_function("js_console_log_number", VOID, &[DOUBLE]);

    // NaN-boxing wrappers (bridge between raw handles and NaN-boxed doubles).
    module.declare_function("js_nanbox_string", DOUBLE, &[I64]);
    module.declare_function("js_nanbox_pointer", DOUBLE, &[I64]);
    module.declare_function("js_nanbox_get_pointer", I64, &[DOUBLE]);

    // Strings (enough to produce string literals for later phases).
    module.declare_function("js_string_from_bytes", I64, &[PTR, I32]);
    module.declare_function("js_string_from_wtf8_bytes", I64, &[PTR, I32]);

    // Type checks.
    module.declare_function("js_is_truthy", I32, &[DOUBLE]);

    // Phase 2.1: timing primitives.
    declare_phase2_1(module);
}

/// Phase 2.1 additions: just `js_date_now()` for in-program timing harnesses.
pub fn declare_phase2_1(module: &mut LlModule) {
    module.declare_function("js_date_now", DOUBLE, &[]);

    // Phase A additions go here too — separate function once they grow.
    declare_phase_a_strings(module);
}

/// Phase A additions: string literal hoisting needs the GC to treat module
/// globals holding string handles as permanent roots. `js_gc_register_global_root`
/// pushes the address into `GLOBAL_ROOTS` (`crates/perry-runtime/src/gc.rs:233`)
/// which the mark phase scans alongside the stack.
pub fn declare_phase_a_strings(module: &mut LlModule) {
    module.declare_function("js_gc_register_global_root", VOID, &[I64]);

    // Phase B (core types) additions live here too — split into a separate
    // function once they grow.
    declare_phase_b_strings(module);
}

/// Phase B string operations.
///
/// `js_string_concat(*const StringHeader, *const StringHeader) -> *mut StringHeader`
/// — both arguments and the return value are raw i64 pointers in our ABI
/// (no NaN-tag). The codegen unboxes the operands by `bitcast double → i64`
/// and `and` with `POINTER_MASK` (0x0000_FFFF_FFFF_FFFF), then re-boxes the
/// result with `js_nanbox_string`.
pub fn declare_phase_b_strings(module: &mut LlModule) {
    module.declare_function("js_string_concat", I64, &[I64, I64]);
    // Dynamic string coercion: takes any NaN-boxed JSValue and returns a
    // raw string handle (`crates/perry-runtime/src/value.rs:813`).
    module.declare_function("js_jsvalue_to_string", I64, &[DOUBLE]);

    // Fused string+value concat (issue #58): collapses js_jsvalue_to_string +
    // js_string_concat into a single allocation for number operands.
    // `js_string_concat_value(prefix_handle, value_f64) -> handle`
    // `js_value_concat_string(value_f64, suffix_handle) -> handle`
    module.declare_function("js_string_concat_value", I64, &[I64, DOUBLE]);
    module.declare_function("js_value_concat_string", I64, &[DOUBLE, I64]);

    // In-place append for the `x = x + y` pattern. When `x` has
    // refcount=1 (unique owner), the runtime mutates in-place and
    // returns the same pointer; otherwise it allocates a new string.
    // Either way the caller must use the returned pointer.
    // (`crates/perry-runtime/src/string.rs:88`)
    module.declare_function("js_string_append", I64, &[I64, I64]);

    // String methods (Phase B.12).
    // All take/return raw i64 string handles. Length args are i32.
    // - js_string_index_of(haystack, needle) -> i32
    // - js_string_index_of_from(haystack, needle, from) -> i32
    // - js_string_slice(s, start, end) -> *mut StringHeader (i64)
    // - js_string_substring(s, start, end) -> *mut StringHeader (i64)
    // - js_string_starts_with(s, prefix) -> i32 (boolean as 0/1)
    // - js_string_ends_with(s, suffix) -> i32
    module.declare_function("js_string_index_of", I32, &[I64, I64]);
    module.declare_function("js_string_index_of_from", I32, &[I64, I64, I32]);
    module.declare_function("js_string_slice", I64, &[I64, I32, I32]);
    module.declare_function("js_string_substring", I64, &[I64, I32, I32]);
    module.declare_function("js_string_split", I64, &[I64, I64]);
    module.declare_function("js_math_pow", DOUBLE, &[DOUBLE, DOUBLE]);

    // Math.* unary functions: use LLVM intrinsics directly so we
    // get hardware instructions / libm calls instead of depending
    // on `js_math_*` runtime symbols (which the auto-optimize
    // dead-strip removes from libperry_runtime.a).
    module.declare_function("llvm.sqrt.f64", DOUBLE, &[DOUBLE]);
    module.declare_function("llvm.floor.f64", DOUBLE, &[DOUBLE]);
    module.declare_function("llvm.ceil.f64", DOUBLE, &[DOUBLE]);
    module.declare_function("llvm.fabs.f64", DOUBLE, &[DOUBLE]);
    module.declare_function("llvm.copysign.f64", DOUBLE, &[DOUBLE, DOUBLE]);
    // Keep js_math_pow for now — Math.pow has overflow / NaN
    // semantics that the libm pow doesn't quite match.

    // JSON.stringify (Phase B.15). The 2-arg form is JsonStringifyFull
    // in the HIR (value, type_hint, indent — actually 3 args; we use the
    // simple 2-arg js_json_stringify for now).
    module.declare_function("js_json_stringify", I64, &[DOUBLE, I32]);

    // Map (Phase B.15). The runtime stores keys/values as NaN-boxed doubles.
    // js_map_alloc returns a *mut MapHeader (i64 pointer).
    module.declare_function("js_map_alloc", I64, &[I32]);
    // typeof: returns a string handle ("number"/"string"/"boolean"/"undefined"/"object"/"function")
    module.declare_function("js_value_typeof", I64, &[DOUBLE]);
    module.declare_function("js_string_starts_with", I32, &[I64, I64]);
    module.declare_function("js_string_ends_with", I32, &[I64, I64]);

    // Closure / function-as-value primitives (Phase D).
    //
    // - js_closure_alloc(func_ptr, capture_count) -> *mut ClosureHeader
    //     Allocates a closure object pointing at the given function with
    //     space for `capture_count` captured-value slots.
    // - js_closure_set/get_capture_f64(closure, idx, value)
    //     Read/write a captured value (NaN-boxed double) at slot `idx`.
    // - js_closure_call0..call16(closure, args…) -> double
    //     Invoke the closure with N args. The runtime extracts the
    //     function pointer from the closure header and calls it with
    //     the closure as the first argument followed by the user args.
    //     The runtime exports js_closure_call0 through js_closure_call16
    //     (see crates/perry-runtime/src/closure.rs); the call site cap in
    //     lower_call.rs matches.
    module.declare_function("js_closure_alloc", I64, &[PTR, I32]);
    module.declare_function("js_closure_set_capture_f64", VOID, &[I64, I32, DOUBLE]);
    module.declare_function("js_closure_get_capture_f64", DOUBLE, &[I64, I32]);
    module.declare_function("js_closure_call0", DOUBLE, &[I64]);
    module.declare_function("js_closure_call1", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_closure_call2", DOUBLE, &[I64, DOUBLE, DOUBLE]);
    module.declare_function("js_closure_call3", DOUBLE, &[I64, DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_closure_call4", DOUBLE, &[I64, DOUBLE, DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_closure_call5", DOUBLE, &[I64, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_closure_call6", DOUBLE, &[I64, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_closure_call7", DOUBLE, &[I64, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_closure_call8", DOUBLE, &[I64, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_closure_call9", DOUBLE, &[I64, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_closure_call10", DOUBLE, &[I64, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_closure_call11", DOUBLE, &[I64, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_closure_call12", DOUBLE, &[I64, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_closure_call13", DOUBLE, &[I64, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_closure_call14", DOUBLE, &[I64, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_closure_call15", DOUBLE, &[I64, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_closure_call16", DOUBLE, &[I64, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE]);

    // Phase B.16 / D follow-ups: more runtime functions discovered
    // by the test-files sweep histogram.
    module.declare_function("js_array_map", I64, &[I64, I64]);
    module.declare_function("js_array_filter", I64, &[I64, I64]);
    module.declare_function("js_array_concat", I64, &[I64, I64]);
    module.declare_function("js_error_new", I64, &[]);
    module.declare_function("js_error_new_with_message", I64, &[I64]);
    module.declare_function("js_map_set", I64, &[I64, DOUBLE, DOUBLE]);
    module.declare_function("js_map_get", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_map_has", I32, &[I64, DOUBLE]);
    module.declare_function("js_map_delete", I32, &[I64, DOUBLE]);
    module.declare_function("js_object_keys", I64, &[I64]);
    module.declare_function("js_is_finite", DOUBLE, &[DOUBLE]);
    module.declare_function("js_is_undefined_or_bare_nan", I32, &[DOUBLE]);
    module.declare_function("js_math_min_array", DOUBLE, &[I64]);
    module.declare_function("js_math_max_array", DOUBLE, &[I64]);
    module.declare_function("js_string_coerce", I64, &[DOUBLE]);
    module.declare_function("js_array_slice", I64, &[I64, I32, I32]);
    module.declare_function("js_array_shift_f64", DOUBLE, &[I64]);
    module.declare_function("js_set_alloc", I64, &[I32]);
    module.declare_function("js_set_from_array", I64, &[I64]);
    module.declare_function("js_map_from_array", I64, &[I64]);
    module.declare_function("js_object_has_property", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_fs_write_file_sync", I32, &[DOUBLE, DOUBLE]);
    module.declare_function("js_fs_exists_sync", I32, &[DOUBLE]);
    // fs.readFileSync(path, encoding) — returns a raw *mut StringHeader i64.
    module.declare_function("js_fs_read_file_sync", I64, &[DOUBLE]);
    // fs.mkdirSync(path) — returns i32 status (1=success).
    module.declare_function("js_fs_mkdir_sync", I32, &[DOUBLE]);
    // fs.unlinkSync(path) — returns i32 status.
    module.declare_function("js_fs_unlink_sync", I32, &[DOUBLE]);
    // fs.readdirSync(path) — returns NaN-boxed array of string names (f64).
    module.declare_function("js_fs_readdir_sync", DOUBLE, &[DOUBLE]);
    // fs.statSync(path) — returns a NaN-boxed object with isFile/isDirectory/size fields.
    module.declare_function("js_fs_stat_sync", DOUBLE, &[DOUBLE]);
    // fs.renameSync(from, to) — returns i32 status.
    module.declare_function("js_fs_rename_sync", I32, &[DOUBLE, DOUBLE]);
    // fs.copyFileSync(from, to) — returns i32 status.
    module.declare_function("js_fs_copy_file_sync", I32, &[DOUBLE, DOUBLE]);
    // fs.accessSync(path) — returns i32 status (1=ok, 0=error).
    module.declare_function("js_fs_access_sync", I32, &[DOUBLE]);
    // fs.accessSync(path) — Node-compatible variant that throws on
    // failure (via js_throw → setjmp longjmp). Returns NaN-boxed undefined.
    module.declare_function("js_fs_access_sync_throw", DOUBLE, &[DOUBLE]);
    // fs.realpathSync(path) — returns raw *mut StringHeader i64.
    module.declare_function("js_fs_realpath_sync", I64, &[DOUBLE]);
    // fs.mkdtempSync(prefix) — returns raw *mut StringHeader i64.
    module.declare_function("js_fs_mkdtemp_sync", I64, &[DOUBLE]);
    // fs.rmdirSync(path) — returns i32 status.
    module.declare_function("js_fs_rmdir_sync", I32, &[DOUBLE]);
    // fs.rmRecursive(path) — recursive remove; returns i32 (1=ok, 0=fail).
    module.declare_function("js_fs_rm_recursive", I32, &[DOUBLE]);
    // fs.createWriteStream(path) — returns NaN-boxed stream object.
    module.declare_function("js_fs_create_write_stream", DOUBLE, &[DOUBLE]);
    // fs.createReadStream(path[, options]) — returns NaN-boxed stream object.
    module.declare_function("js_fs_create_read_stream", DOUBLE, &[DOUBLE]);
    // fs.readFile(path, encoding, callback) — Node-compatible callback variant.
    module.declare_function("js_fs_read_file_callback", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    // Stats helper: method dispatcher called from the LLVM dispatch fast path.
    module.declare_function("js_fs_stats_is_file", DOUBLE, &[DOUBLE]);
    module.declare_function("js_fs_stats_is_directory", DOUBLE, &[DOUBLE]);
    // fs.readFileSync(path) with no encoding — returns a raw *mut BufferHeader
    // that the runtime's format_jsvalue path recognizes via BUFFER_REGISTRY
    // and prints as `<Buffer xx xx ...>`.
    module.declare_function("js_fs_read_file_binary", I64, &[DOUBLE]);
    module.declare_function("js_number_coerce", DOUBLE, &[DOUBLE]);
    module.declare_function("js_set_add", I64, &[I64, DOUBLE]);
    module.declare_function("js_set_has", I32, &[I64, DOUBLE]);
    module.declare_function("js_set_delete", I32, &[I64, DOUBLE]);
    module.declare_function("js_set_size", I32, &[I64]);
    module.declare_function("js_string_to_lower_case", I64, &[I64]);
    module.declare_function("js_string_to_upper_case", I64, &[I64]);
    module.declare_function("js_string_trim", I64, &[I64]);
    module.declare_function("js_string_trim_start", I64, &[I64]);
    module.declare_function("js_string_trim_end", I64, &[I64]);
    module.declare_function("js_string_char_at", I64, &[I64, I32]);
    module.declare_function("js_string_to_char_array", I64, &[I64]);
    module.declare_function("js_string_repeat", I64, &[I64, I32]);
    module.declare_function("js_string_replace_string", I64, &[I64, I64, I64]);
    module.declare_function("js_string_replace_all_string", I64, &[I64, I64, I64]);
    module.declare_function("js_string_equals", I32, &[I64, I64]);
    module.declare_function("js_string_compare", I32, &[I64, I64]);
    module.declare_function("js_jsvalue_to_string_radix", I64, &[DOUBLE, I32]);
    module.declare_function("js_math_random", DOUBLE, &[]);
    module.declare_function("js_console_log_spread", VOID, &[I64]);
    module.declare_function("js_console_error_spread", VOID, &[I64]);
    module.declare_function("js_console_warn_spread", VOID, &[I64]);
    module.declare_function("js_getenv", I64, &[I64]);
    module.declare_function("js_console_table", VOID, &[DOUBLE]);
    module.declare_function("js_console_trace", VOID, &[DOUBLE]);
    // process.* — see `perry-runtime/src/os.rs` and `perry-runtime/src/process.rs`.
    // Most process accessors return raw pointers (I64) that the call site
    // must NaN-box. The ones that return already-boxed f64 values
    // (`js_process_versions`, `js_process_memory_usage`, `js_process_hrtime_bigint`,
    // `js_process_stdin/out/err`) are declared as DOUBLE.
    module.declare_function("js_process_cwd", I64, &[]);
    module.declare_function("js_process_argv", I64, &[]);
    module.declare_function("js_process_pid", DOUBLE, &[]);
    module.declare_function("js_process_ppid", DOUBLE, &[]);
    module.declare_function("js_process_uptime", DOUBLE, &[]);
    module.declare_function("js_process_version", I64, &[]);
    module.declare_function("js_process_versions", DOUBLE, &[]);
    module.declare_function("js_process_memory_usage", DOUBLE, &[]);
    module.declare_function("js_process_env", DOUBLE, &[]);
    module.declare_function("js_process_hrtime_bigint", DOUBLE, &[]);
    module.declare_function("js_process_chdir", VOID, &[I64]);
    module.declare_function("js_process_kill", VOID, &[DOUBLE, DOUBLE]);
    module.declare_function("js_process_exit", VOID, &[DOUBLE]);
    module.declare_function("js_process_on", VOID, &[I64, I64]);
    module.declare_function("js_process_next_tick", VOID, &[I64]);
    module.declare_function("js_process_stdin", DOUBLE, &[]);
    module.declare_function("js_process_stdout", DOUBLE, &[]);
    module.declare_function("js_process_stderr", DOUBLE, &[]);
    // os.* — also used by Expr::OsArch/Type/Platform/Release/Hostname/EOL.
    module.declare_function("js_os_platform", I64, &[]);
    module.declare_function("js_os_arch", I64, &[]);
    module.declare_function("js_os_type", I64, &[]);
    module.declare_function("js_os_release", I64, &[]);
    module.declare_function("js_os_hostname", I64, &[]);
    module.declare_function("js_os_eol", I64, &[]);
    // Heap-allocated mutable capture boxes.
    // See crates/perry-runtime/src/box.rs. These let multiple
    // closures share mutable state (e.g. a counter captured by
    // both inc() and get() in a returned object literal).
    module.declare_function("js_box_alloc", I64, &[DOUBLE]);
    module.declare_function("js_box_get", DOUBLE, &[I64]);
    module.declare_function("js_box_set", VOID, &[I64, DOUBLE]);
    module.declare_function("js_object_get_class_id", I32, &[I64]);
    module.declare_function("js_object_alloc_with_parent", I64, &[I32, I32, I32]);
    // Class instance allocator that pre-populates the keys_array with
    // the class's field names. Required so the LLVM PropertyGet/Set
    // fast path's slot indices match the runtime's by-name dispatch
    // (which walks keys_array). Without this, classes that mix
    // fast-path field access with runtime-helper field access (e.g.
    // PropertySet via fast path + PropertyUpdate via runtime) end up
    // reading/writing different slots for the same field name.
    module.declare_function(
        "js_object_alloc_class_with_keys",
        I64,
        &[I32, I32, I32, PTR, I32],
    );
    // Fast class allocator that takes a pre-built keys_array pointer
    // directly, bypassing the per-call SHAPE_CACHE lookup. The codegen
    // emits one `js_build_class_keys_array` call at module init per
    // class, stores the result in a per-class global, then uses this
    // function on every `new ClassName()` call.
    module.declare_function(
        "js_object_alloc_class_inline_keys",
        I64,
        &[I32, I32, I32, I64],
    );
    module.declare_function(
        "js_build_class_keys_array",
        I64,
        &[I32, I32, PTR, I32],
    );
    // Inline bump-allocator state accessor + slow path. The codegen
    // calls `js_inline_arena_state` once per JS function entry, caches
    // the returned pointer in a stack slot, and reads/writes the
    // bump-pointer state directly via fixed GEPs (data=0, offset=8,
    // size=16). When the bump check fails, it calls
    // `js_inline_arena_slow_alloc` which syncs back to the underlying
    // arena, allocates a new block, and returns the new pointer.
    //
    // The runtime structs live in `crates/perry-runtime/src/arena.rs`.
    // Field offsets are load-bearing — keep `#[repr(C)] InlineArenaState`
    // in sync with the GEPs we emit in `lower_call::compile_new`.
    module.declare_function("js_inline_arena_state", PTR, &[]);
    module.declare_function(
        "js_inline_arena_slow_alloc",
        PTR,
        &[PTR, I64, I64],
    );
    module.declare_function("js_object_delete_field", I32, &[I64, I64]);
    // js_eq takes JSValue (#[repr(transparent)] u64) for both
    // params + return — i64 in the ABI, not double.
    module.declare_function("js_eq", I64, &[I64, I64]);
    module.declare_function("js_loose_eq", I64, &[I64, I64]);
    module.declare_function("js_number_to_fixed", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_string_replace_regex", I64, &[I64, I64, I64]);
    module.declare_function("js_array_at", DOUBLE, &[I64, DOUBLE]);
    // Date getters: all take a timestamp double, return a double.
    module.declare_function("js_date_get_time", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_full_year", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_month", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_date", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_hours", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_minutes", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_seconds", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_milliseconds", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_utc_day", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_utc_full_year", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_utc_month", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_utc_date", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_utc_hours", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_utc_minutes", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_utc_seconds", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_utc_milliseconds", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_value_of", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_get_timezone_offset", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_to_iso_string", I64, &[DOUBLE]);
    module.declare_function("js_date_new_from_timestamp", DOUBLE, &[DOUBLE]);
    module.declare_function("js_date_new_from_value", DOUBLE, &[DOUBLE]);
    module.declare_function("js_array_indexOf_f64", I32, &[I64, DOUBLE]);
    module.declare_function("js_array_includes_f64", I32, &[I64, DOUBLE]);
    module.declare_function("js_array_includes_jsvalue", I32, &[I64, DOUBLE]);
    module.declare_function("js_map_size", I32, &[I64]);
    module.declare_function("js_map_clear", VOID, &[I64]);
    module.declare_function("js_set_clear", VOID, &[I64]);
    // Map iteration: entries/keys/values all take a map pointer and return an array pointer.
    module.declare_function("js_map_entries", I64, &[I64]);
    module.declare_function("js_map_keys", I64, &[I64]);
    module.declare_function("js_map_values", I64, &[I64]);
    // Map/Set forEach: (collection_ptr, callback_nanboxed_f64) -> void
    module.declare_function("js_map_foreach", VOID, &[I64, DOUBLE]);
    module.declare_function("js_set_foreach", VOID, &[I64, DOUBLE]);
    // Set to array conversion (for Set iteration via for...of)
    module.declare_function("js_set_to_array", I64, &[I64]);
    // Splice is unusual: takes an out-pointer for the deleted array
    // and returns the modified-in-place input (the splice point may
    // realloc). Param order is (arr, start, delete_count, items_ptr,
    // items_count, out_arr_ptr).
    module.declare_function("js_array_splice", I64, &[I64, I32, I32, PTR, I32, PTR]);
    module.declare_function("js_parse_int", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_parse_float", DOUBLE, &[I64]);
    module.declare_function("js_array_reduce", DOUBLE, &[I64, I64, I32, DOUBLE]);
    module.declare_function("js_array_reduce_right", DOUBLE, &[I64, I64, I32, DOUBLE]);
    module.declare_function("js_array_sort_default", I64, &[I64]);
    module.declare_function("js_array_reverse", I64, &[I64]);
    module.declare_function("js_array_flat", I64, &[I64]);
    module.declare_function("js_array_flatMap", I64, &[I64, I64]);
    module.declare_function("js_array_sort_with_comparator", I64, &[I64, I64]);
    // ES2023 immutable array methods
    module.declare_function("js_array_to_reversed", I64, &[I64]);
    module.declare_function("js_array_to_sorted_default", I64, &[I64]);
    module.declare_function("js_array_to_sorted_with_comparator", I64, &[I64, I64]);
    module.declare_function("js_array_to_spliced", I64, &[I64, DOUBLE, DOUBLE, PTR, I32]);
    module.declare_function("js_array_with", I64, &[I64, DOUBLE, DOUBLE]);
    module.declare_function("js_array_copy_within", I64, &[I64, DOUBLE, DOUBLE, I32, DOUBLE]);
    module.declare_function("js_regexp_new", I64, &[I64, I64]);
    module.declare_function("js_regexp_test", I32, &[I64, I64]);
    module.declare_function("js_get_string_pointer_unified", I64, &[DOUBLE]);
    module.declare_function("js_bigint_from_string", I64, &[PTR, I32]);
    module.declare_function("js_bigint_from_f64", I64, &[DOUBLE]);
    module.declare_function("js_bigint_cmp", I32, &[I64, I64]);
    // Dynamic bigint arithmetic — lowered from `Expr::Binary` when
    // either operand is statically bigint-typed. These unbox, call
    // the raw `js_bigint_<op>`, and re-box with BIGINT_TAG. Also
    // tolerate mixed bigint/int32 operands.
    module.declare_function("js_dynamic_add", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_dynamic_sub", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_dynamic_mul", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_dynamic_div", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_dynamic_mod", DOUBLE, &[DOUBLE, DOUBLE]);
    // Dynamic bigint bitwise ops — lowered from `Expr::Binary` when
    // either operand is statically bigint-typed. Unbox, call the raw
    // `js_bigint_<op>`, re-box with BIGINT_TAG. Fall through to i32
    // ToInt32 semantics for the pure-number case (closes #39).
    module.declare_function("js_dynamic_bitand", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_dynamic_bitor", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_dynamic_bitxor", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_dynamic_shl", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_dynamic_shr", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_instanceof", DOUBLE, &[DOUBLE, I32]);
    module.declare_function("js_register_class_extends_error", VOID, &[I32]);
    // Inline-allocator class registration: emitted once per class
    // with a parent in the entry-block init prelude. The runtime
    // allocators register on every alloc; the inline allocator skips
    // the alloc-site call and relies on this one-time registration.
    module.declare_function("js_register_class_parent", VOID, &[I32, I32]);
    module.declare_function("js_typeerror_new", I64, &[I64]);
    module.declare_function("js_rangeerror_new", I64, &[I64]);
    module.declare_function("js_syntaxerror_new", I64, &[I64]);
    module.declare_function("js_referenceerror_new", I64, &[I64]);
    // WeakMap / WeakSet / WeakRef / FinalizationRegistry — called
    // via ExternFuncRef from the HIR lowering (which synthesizes
    // `Call(ExternFuncRef("js_weakmap_set"), [...])`). The f64/f64
    // ABI matches both the runtime signature and the codegen's
    // generic extern-call path at lower_call.rs:149.
    module.declare_function("js_weakmap_new", I64, &[]);
    module.declare_function("js_weakset_new", I64, &[]);
    module.declare_function("js_weakmap_set", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_weakmap_get", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_weakmap_has", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_weakmap_delete", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_weakset_add", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_weakset_has", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_weakset_delete", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_weak_throw_primitive", DOUBLE, &[]);
    // Buffer.from(str, encoding) runtime helpers.
    module.declare_function("js_buffer_from_string", I64, &[I64, I32]);
    module.declare_function("js_encoding_tag_from_value", I32, &[DOUBLE]);
    // Universal `.toString(encoding)` dispatch — branches on
    // is_registered_buffer at runtime, falls back to js_jsvalue_to_string.
    module.declare_function("js_value_to_string_with_encoding", I64, &[DOUBLE, I32]);
    module.declare_function("js_fs_unlink_sync", I32, &[DOUBLE]);
    module.declare_function("js_object_values", I64, &[I64]);
    module.declare_function("js_object_entries", I64, &[I64]);
    module.declare_function("js_path_join", I64, &[I64, I64]);
    module.declare_function("js_path_dirname", I64, &[I64]);
    module.declare_function("js_path_relative", I64, &[I64, I64]);
    module.declare_function("js_object_from_entries", DOUBLE, &[DOUBLE]);
    module.declare_function("js_string_match", I64, &[I64, I64]);
    module.declare_function("llvm.log.f64", DOUBLE, &[DOUBLE]);
    module.declare_function("llvm.log2.f64", DOUBLE, &[DOUBLE]);
    module.declare_function("llvm.log10.f64", DOUBLE, &[DOUBLE]);
    module.declare_function("llvm.exp.f64", DOUBLE, &[DOUBLE]);
    module.declare_function("llvm.sin.f64", DOUBLE, &[DOUBLE]);
    module.declare_function("llvm.cos.f64", DOUBLE, &[DOUBLE]);
    module.declare_function("js_path_basename", I64, &[I64]);
    module.declare_function("js_path_basename_ext", I64, &[I64, I64]);
    module.declare_function("js_path_extname", I64, &[I64]);
    module.declare_function("js_path_sep_get", I64, &[]);
    module.declare_function("js_path_delimiter_get", I64, &[]);
    module.declare_function("js_path_parse", I64, &[I64]);
    // JSON.parse returns JSValue (u64) via integer register on ARM64,
    // not f64. Use I64 return + bitcast to avoid ABI mismatch crash.
    module.declare_function("js_json_parse", I64, &[I64]);
    // JSON.parse<T[]> schema-directed parse: same return semantics.
    // Args: text_ptr (i64), packed_keys (i64), packed_keys_len (i32),
    // field_count (i32).
    module.declare_function("js_json_parse_typed_array", I64, &[I64, I64, I32, I32]);
    // Date string formatters
    module.declare_function("js_date_to_date_string", I64, &[DOUBLE]);
    module.declare_function("js_date_to_time_string", I64, &[DOUBLE]);
    module.declare_function("js_date_to_locale_date_string", I64, &[DOUBLE]);
    module.declare_function("js_date_to_locale_time_string", I64, &[DOUBLE]);
    module.declare_function("js_date_to_json", I64, &[DOUBLE]);
    // RegExp exec
    module.declare_function("js_regexp_exec", I64, &[I64, I64]);
    module.declare_function("js_number_to_precision", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_number_to_exponential", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_date_new", DOUBLE, &[]);
    module.declare_function("js_number_is_integer", DOUBLE, &[DOUBLE]);
    module.declare_function("js_number_is_nan", DOUBLE, &[DOUBLE]);
    module.declare_function("js_number_is_safe_integer", DOUBLE, &[DOUBLE]);
    // Date parsing / UTC constructors / UTC setters.
    module.declare_function("js_date_parse", DOUBLE, &[I64]);
    module.declare_function("js_date_utc", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_date_set_utc_full_year", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_date_set_utc_month", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_date_set_utc_date", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_date_set_utc_hours", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_date_set_utc_minutes", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_date_set_utc_seconds", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_date_set_utc_milliseconds", DOUBLE, &[DOUBLE, DOUBLE]);
    // Math extras (stubs in expr.rs had fallen through to no-op/passthrough).
    module.declare_function("js_math_clz32", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_cbrt", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_fround", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_sinh", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_cosh", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_tanh", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_asinh", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_acosh", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_atanh", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_hypot", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_object_is", DOUBLE, &[DOUBLE, DOUBLE]);
    // Path + URI (wired in expr.rs; runtime already implemented).
    module.declare_function("js_path_normalize", I64, &[I64]);
    module.declare_function("js_path_format", I64, &[DOUBLE]);
    module.declare_function("js_path_is_absolute", I32, &[I64]);
    module.declare_function("js_encode_uri", I64, &[DOUBLE]);
    module.declare_function("js_decode_uri", I64, &[DOUBLE]);
    module.declare_function("js_encode_uri_component", I64, &[DOUBLE]);
    module.declare_function("js_decode_uri_component", I64, &[DOUBLE]);
    // TextEncoder / TextDecoder — LLVM variant uses an ArrayHeader-backed
    // buffer (see `crates/perry-runtime/src/text.rs`). Encode returns an
    // i64 pointing at an ArrayHeader with f64 elements (one per UTF-8
    // byte). Decode accepts both ArrayHeader (from encode) and
    // BufferHeader (from `new Uint8Array([...])`).
    module.declare_function("js_text_encoder_new", I64, &[]);
    module.declare_function("js_text_decoder_new", I64, &[]);
    module.declare_function("js_text_encoder_encode_llvm", I64, &[DOUBLE]);
    module.declare_function("js_text_decoder_decode_llvm", I64, &[DOUBLE]);
    // Microtask queue (queueMicrotask / process.nextTick).
    module.declare_function("js_queue_microtask", VOID, &[I64]);
    module.declare_function("js_drain_queued_microtasks", VOID, &[]);
    // Uint8Array constructor wrapper that flags the resulting buffer so the
    // formatter prints `Uint8Array(N) [ ... ]` instead of `<Buffer ...>`.
    module.declare_function("js_uint8array_from_array", I64, &[I64]);
    // `new Uint8Array(x)` runtime dispatch — handles the non-literal case
    // where `x` could be a number (length) or an array (source data).
    module.declare_function("js_uint8array_new", I64, &[DOUBLE]);
    // Generic typed array runtime (Int8/16/32, Uint16/32, Float32/64, Uint8Clamped).
    // Uint8Array piggybacks on the BufferHeader path.
    module.declare_function("js_typed_array_new_empty", I64, &[I32, I32]);
    module.declare_function("js_typed_array_new_from_array", I64, &[I32, I64]);
    // Runtime-dispatched constructor: handles numeric length OR source-array arg.
    module.declare_function("js_typed_array_new", I64, &[I32, DOUBLE]);
    module.declare_function("js_typed_array_length", I32, &[I64]);
    module.declare_function("js_typed_array_get", DOUBLE, &[I64, I32]);
    module.declare_function("js_typed_array_at", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_typed_array_set", VOID, &[I64, I32, DOUBLE]);
    module.declare_function("js_typed_array_to_reversed", I64, &[I64]);
    module.declare_function("js_typed_array_to_sorted_default", I64, &[I64]);
    module.declare_function("js_typed_array_to_sorted_with_comparator", I64, &[I64, I64]);
    module.declare_function("js_typed_array_with", I64, &[I64, DOUBLE, DOUBLE]);
    module.declare_function("js_typed_array_find_last", DOUBLE, &[I64, I64]);
    module.declare_function("js_typed_array_find_last_index", DOUBLE, &[I64, I64]);
    // Object introspection / mutation (Agent A's accessor-descriptor work).
    module.declare_function("js_object_has_own", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_object_define_property", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_object_get_own_property_descriptor", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_object_get_own_property_names", DOUBLE, &[DOUBLE]);
    // Symbol runtime (perry-runtime/src/symbol.rs)
    module.declare_function("js_symbol_new", DOUBLE, &[DOUBLE]);
    module.declare_function("js_symbol_new_empty", DOUBLE, &[]);
    module.declare_function("js_symbol_for", DOUBLE, &[DOUBLE]);
    module.declare_function("js_symbol_key_for", DOUBLE, &[DOUBLE]);
    module.declare_function("js_symbol_description", DOUBLE, &[DOUBLE]);
    module.declare_function("js_symbol_to_string", I64, &[DOUBLE]);
    module.declare_function("js_symbol_equals", I32, &[DOUBLE, DOUBLE]);
    module.declare_function("js_is_symbol", I32, &[DOUBLE]);
    module.declare_function("js_object_get_own_property_symbols", I64, &[DOUBLE]);
    module.declare_function("js_object_set_symbol_property", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_object_get_symbol_property", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_object_create", DOUBLE, &[DOUBLE]);
    module.declare_function("js_object_freeze", DOUBLE, &[DOUBLE]);
    module.declare_function("js_object_seal", DOUBLE, &[DOUBLE]);
    module.declare_function("js_object_prevent_extensions", DOUBLE, &[DOUBLE]);
    // Object spread: copy all own fields from src into dst.
    module.declare_function("js_object_copy_own_fields", VOID, &[I64, DOUBLE]);
    // String extras (already in string.rs; expr.rs was stubbing or missing dispatch).
    module.declare_function("js_string_at", DOUBLE, &[I64, I32]);
    module.declare_function("js_string_code_point_at", DOUBLE, &[I64, I32]);
    module.declare_function("js_string_from_code_point", I64, &[I32]);
    module.declare_function("js_string_from_char_code", I64, &[I32]);
    module.declare_function("js_string_char_code_at", DOUBLE, &[I64, I32]);
    module.declare_function("js_string_last_index_of", I32, &[I64, I64]);
    module.declare_function("js_string_locale_compare", DOUBLE, &[I64, I64]);
    module.declare_function("js_string_normalize", I64, &[I64, I64]);
    module.declare_function("js_string_pad_start", I64, &[I64, I32, I64]);
    module.declare_function("js_string_pad_end", I64, &[I64, I32, I64]);
    module.declare_function("js_string_is_well_formed", DOUBLE, &[I64]);
    module.declare_function("js_string_to_well_formed", I64, &[I64]);
    module.declare_function("js_string_match_all", I64, &[I64, I64]);
    module.declare_function("js_string_search_regex", I32, &[I64, I64]);
    // Regex extras (runtime has them; codegen was stubbing).
    module.declare_function("js_regexp_exec_get_index", DOUBLE, &[]);
    module.declare_function("js_regexp_exec_get_groups", I64, &[]);
    module.declare_function("js_regexp_get_last_index", DOUBLE, &[I64]);
    module.declare_function("js_regexp_set_last_index", VOID, &[I64, DOUBLE]);
    module.declare_function("js_regexp_get_source", I64, &[I64]);
    module.declare_function("js_regexp_get_flags", I64, &[I64]);
    module.declare_function("js_string_replace_regex_named", I64, &[I64, I64, I64]);
    module.declare_function("js_string_replace_regex_fn", I64, &[I64, I64, DOUBLE]);
    // structuredClone(v) — real deep copy, was stubbed as passthrough.
    module.declare_function("js_structured_clone", DOUBLE, &[DOUBLE]);
    // WeakRef / FinalizationRegistry (weakref.rs). `js_weakref_new` /
    // `js_finreg_new` return raw `*mut ObjectHeader` (i64 pointer, must be
    // POINTER_TAG-boxed at the call site). The deref/register/unregister
    // helpers already return NaN-tagged f64 values.
    module.declare_function("js_weakref_new", I64, &[DOUBLE]);
    module.declare_function("js_weakref_deref", DOUBLE, &[DOUBLE]);
    module.declare_function("js_finreg_new", I64, &[DOUBLE]);
    module.declare_function("js_finreg_register", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_finreg_unregister", DOUBLE, &[DOUBLE, DOUBLE]);
    // atob/btoa: base64 decode/encode. Take a NaN-boxed string (f64),
    // return a raw *const StringHeader (i64, must be STRING_TAG-boxed).
    module.declare_function("js_atob", I64, &[DOUBLE]);
    module.declare_function("js_btoa", I64, &[DOUBLE]);
    module.declare_function("js_object_is_frozen", DOUBLE, &[DOUBLE]);
    module.declare_function("js_object_is_sealed", DOUBLE, &[DOUBLE]);
    module.declare_function("js_object_is_extensible", DOUBLE, &[DOUBLE]);
    // Error subclasses (Agent B's runtime work).
    module.declare_function("js_aggregateerror_new", I64, &[I64, I64]);
    module.declare_function("js_error_new_with_cause", I64, &[I64, DOUBLE]);
    // AggregateError.errors field access — returns raw *ArrayHeader.
    module.declare_function("js_error_get_errors", I64, &[I64]);
    // Crypto stdlib — sha256/md5/hmac/randomBytes/randomUUID used by
    // the expr.rs chain collapse for createHash().update().digest().
    module.declare_function("js_crypto_sha256", I64, &[I64]);
    module.declare_function("js_crypto_sha256_bytes", I64, &[I64]);
    module.declare_function("js_crypto_md5", I64, &[I64]);
    module.declare_function("js_crypto_hmac_sha256", I64, &[I64, I64]);
    module.declare_function("js_crypto_hmac_sha256_bytes", I64, &[I64, I64]);
    module.declare_function("js_crypto_pbkdf2_bytes", I64, &[I64, I64, DOUBLE, DOUBLE]);
    module.declare_function("js_crypto_random_bytes_buffer", I64, &[DOUBLE]);
    module.declare_function("js_crypto_random_uuid", I64, &[]);
    // Hash-handle form (issue #86): `const h = crypto.createHash(alg);
    // h.update(x); h.digest()`. Returns a NaN-boxed POINTER_TAG handle id;
    // subsequent method dispatch flows through HANDLE_METHOD_DISPATCH.
    module.declare_function("js_crypto_create_hash", DOUBLE, &[I64]);
    module.declare_function("js_string_from_bytes", I64, &[I64, I32]);
    module.declare_function("js_string_from_wtf8_bytes", I64, &[I64, I32]);
    // Buffer.alloc(size, fill) — returns raw *mut BufferHeader.
    module.declare_function("js_buffer_alloc", I64, &[I32, I32]);
    // JSON full-featured stringify/parse (replacer + indent + reviver).
    module.declare_function("js_json_stringify_full", I64, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_json_parse_with_reviver", I64, &[I64, I64]);
    module.declare_function("js_array_find", DOUBLE, &[I64, I64]);
    module.declare_function("js_array_findIndex", I32, &[I64, I64]);
    module.declare_function("js_array_find_last", DOUBLE, &[I64, I64]);
    module.declare_function("js_array_find_last_index", I32, &[I64, I64]);
    module.declare_function("js_array_some", DOUBLE, &[I64, I64]);
    module.declare_function("js_array_every", DOUBLE, &[I64, I64]);

    // Phase E: async/await runtime support.
    // Promise polling: state is 0=pending, 1=fulfilled, 2=rejected.
    // The await busy-wait loop polls js_promise_state, calls
    // js_promise_run_microtasks + js_sleep_ms while pending, then
    // pulls the value via js_promise_value (or reason via
    // js_promise_reason on rejection).
    module.declare_function("js_promise_state", I32, &[I64]);
    module.declare_function("js_promise_value", DOUBLE, &[I64]);
    module.declare_function("js_promise_reason", DOUBLE, &[I64]);
    // Safe guard used by `Expr::Await` to detect non-promise
    // operands before unboxing. Takes a NaN-boxed f64, returns
    // 1 if it points at a GC_TYPE_PROMISE allocation else 0.
    module.declare_function("js_value_is_promise", I32, &[DOUBLE]);
    module.declare_function("js_promise_run_microtasks", I32, &[]);
    // Drain stdlib's tokio async queue (fetch, DB, etc.). Lives in
    // perry-runtime as a thin function-pointer trampoline so it's
    // safe to call even when perry-stdlib is not linked (no-op).
    module.declare_function("js_run_stdlib_pump", VOID, &[]);
    module.declare_function("js_sleep_ms", VOID, &[DOUBLE]);
    // Issue #84: condvar-backed wait for the event loop / await busy-wait.
    // Replaces fixed-quantum `js_sleep_ms(10.0)` / `js_sleep_ms(1.0)`.
    // Returns immediately when a tokio worker calls js_notify_main_thread()
    // after enqueueing onto a queue the pump drains; otherwise sleeps until
    // the next timer deadline (or 1s safety cap).
    module.declare_function("js_wait_for_event", VOID, &[]);
    module.declare_function("js_throw", VOID, &[DOUBLE]);

    // Exception handling (Phase G): setjmp/longjmp-based try/catch.
    // js_try_push() returns a ptr to a jmp_buf.
    // setjmp(ptr) returns i32 (0 on first call, non-0 after longjmp).
    // js_try_end() pops the try depth (no return value).
    // js_get_exception() returns the thrown NaN-boxed value.
    // js_clear_exception() resets the exception state.
    // js_has_exception() returns i32 (1 if exception is active, 0 otherwise).
    // js_enter_finally() / js_leave_finally() bracket finally blocks.
    module.declare_function("js_try_push", PTR, &[]);
    // Windows MSVC uses _setjmp(buf, frame_ptr); Unix uses setjmp(buf).
    if cfg!(target_os = "windows") {
        module.declare_function("_setjmp", I32, &[PTR, PTR]);
    } else {
        module.declare_function("setjmp", I32, &[PTR]);
    }
    module.declare_function("js_try_end", VOID, &[]);
    module.declare_function("js_get_exception", DOUBLE, &[]);
    module.declare_function("js_clear_exception", VOID, &[]);
    module.declare_function("js_has_exception", I32, &[]);
    module.declare_function("js_enter_finally", VOID, &[]);
    module.declare_function("js_leave_finally", VOID, &[]);
    module.declare_function("js_await_any_promise", DOUBLE, &[DOUBLE]);
    module.declare_function("js_promise_new", I64, &[]);
    module.declare_function("js_promise_new_with_executor", I64, &[I64]);
    // Timer tick functions — called from the Await busy-wait loop so
    // `setTimeout(resolve, N)` inside a Promise executor actually fires.
    module.declare_function("js_timer_tick", I32, &[]);
    module.declare_function("js_callback_timer_tick", I32, &[]);
    module.declare_function("js_interval_timer_tick", I32, &[]);
    // Timer has-pending checks — called from the main event loop to
    // decide whether to keep ticking or exit.
    module.declare_function("js_timer_has_pending", I32, &[]);
    module.declare_function("js_callback_timer_has_pending", I32, &[]);
    module.declare_function("js_interval_timer_has_pending", I32, &[]);
    // Stdlib has-active-handles — returns 1 if WS servers, pending
    // HTTP events, etc. need the loop to keep running.
    module.declare_function("js_stdlib_has_active_handles", I32, &[]);
    module.declare_function("js_set_timeout_callback", I64, &[I64, DOUBLE]);
    module.declare_function("setInterval", I64, &[I64, DOUBLE]);
    module.declare_function("clearTimeout", VOID, &[I64]);
    module.declare_function("clearInterval", VOID, &[I64]);
    module.declare_function("js_buffer_from_array", I64, &[I64]);
    module.declare_function("js_buffer_length", I32, &[I64]);
    module.declare_function("js_buffer_get", I32, &[I64, I32]);
    // console.time/count runtime functions.
    module.declare_function("js_console_time", VOID, &[I64]);
    module.declare_function("js_console_time_end", VOID, &[I64]);
    module.declare_function("js_console_time_log", VOID, &[I64]);
    module.declare_function("js_console_count", VOID, &[I64]);
    module.declare_function("js_console_count_reset", VOID, &[I64]);
    module.declare_function("js_console_group_begin", VOID, &[]);
    module.declare_function("js_console_group_end", VOID, &[]);
    module.declare_function("js_console_clear", VOID, &[]);
    // Universal PropertyGet method dispatch fallback — routes
    // `recv.method(args)` to the runtime's dispatcher when no static
    // codegen path fires. Used by Map/Set methods on plain object fields.
    module.declare_function(
        "js_native_call_method",
        DOUBLE,
        &[DOUBLE, PTR, I64, PTR, I64],
    );
    module.declare_function("js_promise_resolve", VOID, &[I64, DOUBLE]);
    module.declare_function("js_promise_reject", VOID, &[I64, DOUBLE]);
    module.declare_function("js_promise_resolved", I64, &[DOUBLE]);
    module.declare_function("js_promise_rejected", I64, &[DOUBLE]);
    module.declare_function("js_promise_then", I64, &[I64, I64, I64]);
    module.declare_function("js_promise_all", I64, &[I64]);
    module.declare_function("js_promise_race", I64, &[I64]);
    module.declare_function("js_promise_any", I64, &[I64]);
    module.declare_function("js_promise_all_settled", I64, &[I64]);
    module.declare_function("js_promise_with_resolvers", I64, &[]);
    module.declare_function("js_array_unshift_f64", I64, &[I64, DOUBLE]);
    module.declare_function("js_array_entries", I64, &[I64]);
    module.declare_function("js_array_keys", I64, &[I64]);
    module.declare_function("js_array_values", I64, &[I64]);

    // ──────────────────────────────────────────────────────────────────
    // Web Fetch API: Response / Headers / Request constructors +
    // response body methods + static factories. These are in
    // `crates/perry-stdlib/src/fetch.rs`. Handles flow as plain numeric
    // f64 values (not NaN-boxed) so codegen passes them as DOUBLE.
    // Where the runtime takes i64 (e.g. js_fetch_response_status),
    // codegen converts via fptosi.
    // ──────────────────────────────────────────────────────────────────
    // new Response(body_ptr, status, status_text_ptr, headers_handle) -> f64
    module.declare_function("js_response_new", DOUBLE, &[I64, DOUBLE, I64, DOUBLE]);
    // new Headers() -> f64
    module.declare_function("js_headers_new", DOUBLE, &[]);
    // headers.set(handle_f64, key_ptr, val_ptr) -> f64 (undefined-tag)
    module.declare_function("js_headers_set", DOUBLE, &[DOUBLE, I64, I64]);
    // headers.get(handle_f64, key_ptr) -> *mut StringHeader (i64)
    module.declare_function("js_headers_get", I64, &[DOUBLE, I64]);
    // headers.has(handle_f64, key_ptr) -> f64 (TAG_TRUE/FALSE)
    module.declare_function("js_headers_has", DOUBLE, &[DOUBLE, I64]);
    // headers.delete(handle_f64, key_ptr) -> f64 (undefined-tag)
    module.declare_function("js_headers_delete", DOUBLE, &[DOUBLE, I64]);
    // headers.forEach(handle_f64, cb_nanbox) -> f64 (undefined-tag)
    module.declare_function("js_headers_for_each", DOUBLE, &[DOUBLE, DOUBLE]);

    // new Request(url_ptr, method_ptr, body_ptr, headers_handle_f64) -> f64
    module.declare_function("js_request_new", DOUBLE, &[I64, I64, I64, DOUBLE]);
    module.declare_function("js_request_get_url", I64, &[DOUBLE]);
    module.declare_function("js_request_get_method", I64, &[DOUBLE]);
    module.declare_function("js_request_get_body", DOUBLE, &[DOUBLE]);

    // Response body getters
    module.declare_function("js_fetch_response_status", DOUBLE, &[I64]);
    module.declare_function("js_fetch_response_status_text", I64, &[I64]);
    module.declare_function("js_fetch_response_ok", DOUBLE, &[I64]);
    module.declare_function("js_fetch_response_text", I64, &[I64]);
    module.declare_function("js_fetch_response_json", I64, &[I64]);
    // response.headers / .clone() / .arrayBuffer() / .blob() — all take
    // the f64 response handle.
    module.declare_function("js_response_get_headers", DOUBLE, &[DOUBLE]);
    module.declare_function("js_response_clone", DOUBLE, &[DOUBLE]);
    module.declare_function("js_response_array_buffer", I64, &[DOUBLE]);
    module.declare_function("js_response_blob", I64, &[DOUBLE]);
    // Static factories.
    module.declare_function("js_response_static_json", DOUBLE, &[DOUBLE]);
    module.declare_function("js_response_static_redirect", DOUBLE, &[I64, DOUBLE]);

    // ──────────────────────────────────────────────────────────────────
    // AbortController / AbortSignal — perry-runtime/src/url.rs.
    // Returns *mut ObjectHeader (i64 pointer) — codegen NaN-boxes with
    // POINTER_TAG so regular property get can read fields.
    // ──────────────────────────────────────────────────────────────────
    module.declare_function("js_abort_controller_new", I64, &[]);
    module.declare_function("js_abort_controller_signal", I64, &[I64]);
    module.declare_function("js_abort_controller_abort", VOID, &[I64]);
    module.declare_function("js_abort_controller_abort_reason", VOID, &[I64, DOUBLE]);
    module.declare_function("js_abort_signal_add_listener", VOID, &[I64, DOUBLE, DOUBLE]);
    module.declare_function("js_abort_signal_timeout", I64, &[DOUBLE]);

    declare_phase_b_arrays(module);
}

/// Phase B array operations (number-typed arrays for the first slice).
///
/// All arrays are stored as raw i64 pointers at the runtime level. The
/// codegen NaN-boxes them with `POINTER_TAG` for storage in locals/params,
/// and unboxes back to raw i64 (`bitcast` + `and POINTER_MASK`) before
/// passing to runtime functions.
///
/// - `js_array_alloc(u32) -> *mut ArrayHeader` — allocate with capacity
/// - `js_array_push_f64(arr, value) -> arr*` — push element, may realloc
///   and return a NEW pointer that the caller must use going forward
/// - `js_array_get_f64(arr, index) -> f64` — read typed-number element
/// - `js_array_length(arr) -> u32` — length (u32, sitofp'd to double for
///   our number ABI)
pub fn declare_phase_b_arrays(module: &mut LlModule) {
    module.declare_function("js_array_alloc", I64, &[I32]);
    // Exact-sized literal allocator — one call + N direct stores replaces
    // alloc + N×push_f64. See `js_array_alloc_literal` in perry-runtime/src/array.rs.
    module.declare_function("js_array_alloc_literal", I64, &[I32]);
    module.declare_function("js_array_push_f64", I64, &[I64, DOUBLE]);
    module.declare_function("js_array_get_f64", DOUBLE, &[I64, I32]);
    module.declare_function("js_array_set_f64", VOID, &[I64, I32, DOUBLE]);
    // Extending variant: returns a possibly-realloc'd pointer that the
    // caller must write back to the local slot.
    module.declare_function("js_array_set_f64_extend", I64, &[I64, I32, DOUBLE]);
    module.declare_function("js_array_length", I32, &[I64]);
    // Array.isArray runtime dispatch for values with indeterminate
    // static type (e.g. JSON.parse results, closure captures, any/
    // unknown-typed locals). Returns NaN-boxed boolean.
    module.declare_function("js_array_is_array", DOUBLE, &[DOUBLE]);
    // Issue #73: safe `.length` dispatch by runtime type. Fallback
    // for the inline PropertyGet length path when the GC-type check
    // can't prove the receiver is an Array/String.
    module.declare_function("js_value_length_f64", DOUBLE, &[DOUBLE]);

    // Shadow stack for precise root tracking (gen-GC Phase A per
    // docs/generational-gc-plan.md). Declared now so codegen can
    // reference them; emission at function entry/exit + safepoints
    // is the next milestone.
    //   js_shadow_frame_push(slot_count: u32) -> u64 (frame handle)
    //   js_shadow_frame_pop(frame_handle: u64)
    //   js_shadow_slot_set(idx: u32, value: u64)
    module.declare_function("js_shadow_frame_push", I64, &[I32]);
    module.declare_function("js_shadow_frame_pop", VOID, &[I64]);
    module.declare_function("js_shadow_slot_set", VOID, &[I32, I64]);

    // Write barrier for the generational GC (Phase C per the
    // gen-GC plan). Called by codegen-emitted heap-store sites
    // when sub-phase C2 wires the emission. Records old→young
    // pointer stores in the per-thread remembered set so minor
    // GC can scan precise roots + RS instead of the full old-gen.
    //   js_write_barrier(parent_bits: u64, child_bits: u64)
    module.declare_function("js_write_barrier", VOID, &[I64, I64]);

    // Array methods (Phase B.12).
    // - js_array_pop_f64(arr) -> f64    (last element, NaN if empty)
    // - js_array_join(arr, sep) -> *mut StringHeader (i64)
    module.declare_function("js_array_pop_f64", DOUBLE, &[I64]);
    module.declare_function("js_array_join", I64, &[I64, I64]);
    module.declare_function("js_array_forEach", VOID, &[I64, I64]);
    module.declare_function("js_array_fill", I64, &[I64, DOUBLE]);
    module.declare_function("js_array_delete", I32, &[I64, I32]);
    // Array.from() — js_array_clone handles arrays, Sets, and Maps.
    module.declare_function("js_array_clone", I64, &[I64]);
    // Generator / iterator protocol: walk `.next()`/`.value` loop and collect into array.
    module.declare_function("js_iterator_to_array", I64, &[DOUBLE]);

    declare_phase_b_objects(module);
}

/// Phase B object operations (basic object literals + property get/set).
///
/// - `js_object_alloc(class_id, field_count) -> *mut ObjectHeader` —
///   allocate with class_id=0 for anonymous object literals. The runtime
///   pre-allocates at least 8 inline slots regardless of field_count
///   (`crates/perry-runtime/src/object.rs:500`) to prevent buffer
///   overflow on later set_field calls.
/// - `js_object_set_field_by_name(obj, key, value)` — set field by string
///   key. Both `obj` and `key` are raw i64 pointers; `value` is a
///   NaN-boxed double.
/// - `js_object_get_field_by_name_f64(obj, key) -> f64` — read field by
///   string key, returning the raw f64 (or the NaN-boxed value for
///   non-number fields — same bit pattern, just interpreted differently).
///
/// Field name strings are sourced from the same StringPool the literal
/// strings use, so `obj.x` and `obj["x"]` and `let s = "x"; obj[s]` all
/// share one allocation per unique key.
///
/// The inline bump allocator now handles most object allocation directly;
/// `js_object_alloc(0, N)` is the fallback for dynamic cases.
pub fn declare_phase_b_objects(module: &mut LlModule) {
    module.declare_function("js_object_alloc", I64, &[I32, I32]);
    module.declare_function("js_object_set_field_by_name", VOID, &[I64, I64, DOUBLE]);
    module.declare_function("js_object_get_field_by_name_f64", DOUBLE, &[I64, I64]);
    module.declare_function("js_object_get_field_ic_miss", DOUBLE, &[I64, I64, PTR]);
    // Object rest destructuring: copy all properties from src except excluded keys.
    // Takes a src object ptr and an array of NaN-boxed strings (the excluded keys),
    // returns a new object pointer.
    module.declare_function("js_object_rest", I64, &[I64, I64]);
    // Array alloc variant that pre-sets length to N (for exclude_keys array filling).
    module.declare_function("js_array_alloc_with_length", I64, &[I32]);
    // Unchecked array set (plain array, no buffer/Set/Map dispatch).
    module.declare_function("js_array_set_f64_unchecked", VOID, &[I64, I32, DOUBLE]);

    // --- Proxy / Reflect ---
    module.declare_function("js_proxy_new", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_proxy_revoke", VOID, &[DOUBLE]);
    module.declare_function("js_proxy_is_revoked", I32, &[DOUBLE]);
    module.declare_function("js_proxy_is_proxy", I32, &[DOUBLE]);
    module.declare_function("js_proxy_target", DOUBLE, &[DOUBLE]);
    module.declare_function("js_proxy_get", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_proxy_set", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_proxy_has", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_proxy_delete", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_proxy_apply", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_proxy_construct", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_reflect_get", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_reflect_set", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_reflect_has", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_reflect_delete", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_reflect_own_keys", DOUBLE, &[DOUBLE]);
    module.declare_function("js_reflect_apply", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_reflect_define_property", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);

    declare_stdlib_ffi(module);
}

/// Stdlib / FFI runtime functions. Without these declarations, user code
/// that touches any of the third-party stdlib modules (http, mysql2, pg,
/// redis, mongodb, bcrypt, jsonwebtoken, axios, sharp, cron, WebSocket,
/// zlib, etc.) emits `use of undefined value '@js_*'` at clang -c time
/// because the IR references the name without a preceding `declare`.
///
/// Signatures cross-checked against `crates/perry-runtime/src/` and
/// `crates/perry-stdlib/src/`.
pub fn declare_stdlib_ffi(module: &mut LlModule) {
    // ========== HTTP server ==========
    module.declare_function("js_http_client_request_end", I64, &[I64, DOUBLE]);
    module.declare_function("js_http_client_request_write", I64, &[I64, DOUBLE]);
    module.declare_function("js_http_get", I64, &[DOUBLE, I64]);
    module.declare_function("js_http_on", I64, &[I64, I64, I64]);
    module.declare_function("js_http_request", I64, &[DOUBLE, I64]);
    module.declare_function("js_http_request_body", I64, &[I64]);
    module.declare_function("js_http_request_body_length", DOUBLE, &[I64]);
    module.declare_function("js_http_request_content_type", I64, &[I64]);
    module.declare_function("js_http_request_has_header", DOUBLE, &[I64, I64]);
    module.declare_function("js_http_request_header", I64, &[I64, I64]);
    module.declare_function("js_http_request_headers_all", I64, &[I64]);
    module.declare_function("js_http_request_id", DOUBLE, &[I64]);
    module.declare_function("js_http_request_is_method", DOUBLE, &[I64, I64]);
    module.declare_function("js_http_request_method", I64, &[I64]);
    module.declare_function("js_http_request_path", I64, &[I64]);
    module.declare_function("js_http_request_query", I64, &[I64]);
    module.declare_function("js_http_request_query_all", I64, &[I64]);
    module.declare_function("js_http_request_query_param", I64, &[I64, I64]);
    module.declare_function("js_http_respond_error", DOUBLE, &[I64, DOUBLE, I64]);
    module.declare_function("js_http_respond_html", DOUBLE, &[I64, DOUBLE, I64]);
    module.declare_function("js_http_respond_json", DOUBLE, &[I64, DOUBLE, I64]);
    module.declare_function("js_http_respond_not_found", DOUBLE, &[I64]);
    module.declare_function("js_http_respond_redirect", DOUBLE, &[I64, I64, DOUBLE]);
    module.declare_function("js_http_respond_status_text", I64, &[DOUBLE]);
    module.declare_function("js_http_respond_text", DOUBLE, &[I64, DOUBLE, I64]);
    module.declare_function("js_http_respond_with_headers", DOUBLE, &[I64, DOUBLE, I64, I64]);
    module.declare_function("js_http_response_headers", DOUBLE, &[I64]);
    module.declare_function("js_http_server_accept_v2", I64, &[I64]);
    module.declare_function("js_http_server_close", DOUBLE, &[I64]);
    module.declare_function("js_http_server_create", I64, &[DOUBLE]);
    module.declare_function("js_http_set_header", I64, &[I64, I64, I64]);
    module.declare_function("js_http_set_timeout", I64, &[I64, DOUBLE]);
    module.declare_function("js_http_status_code", DOUBLE, &[I64]);
    module.declare_function("js_http_status_message", I64, &[I64]);

    // ========== HTTPS ==========
    module.declare_function("js_https_get", I64, &[DOUBLE, I64]);
    module.declare_function("js_https_request", I64, &[DOUBLE, I64]);

    // ========== PostgreSQL (pg) ==========
    module.declare_function("js_pg_client_connect", I64, &[I64]);
    module.declare_function("js_pg_client_end", I64, &[I64]);
    module.declare_function("js_pg_client_new", I64, &[I64]);
    module.declare_function("js_pg_client_query", I64, &[I64, I64]);
    module.declare_function("js_pg_client_query_params", I64, &[I64, I64, I64]);
    module.declare_function("js_pg_connect", I64, &[I64]);
    module.declare_function("js_pg_create_pool", I64, &[I64]);
    module.declare_function("js_pg_pool_end", I64, &[I64]);
    module.declare_function("js_pg_pool_new", I64, &[I64]);
    module.declare_function("js_pg_pool_query", I64, &[I64, I64]);

    // ========== Redis / ioredis ==========
    module.declare_function("js_ioredis_connect", I64, &[I64]);
    module.declare_function("js_ioredis_decr", I64, &[I64, I64]);
    module.declare_function("js_ioredis_del", I64, &[I64, I64]);
    module.declare_function("js_ioredis_disconnect", VOID, &[I64]);
    module.declare_function("js_ioredis_exists", I64, &[I64, I64]);
    module.declare_function("js_ioredis_expire", I64, &[I64, I64, DOUBLE]);
    module.declare_function("js_ioredis_get", I64, &[I64, I64]);
    module.declare_function("js_ioredis_hdel", I64, &[I64, I64, I64]);
    module.declare_function("js_ioredis_hget", I64, &[I64, I64, I64]);
    module.declare_function("js_ioredis_hgetall", I64, &[I64, I64]);
    module.declare_function("js_ioredis_hlen", I64, &[I64, I64]);
    module.declare_function("js_ioredis_hset", I64, &[I64, I64, I64, I64]);
    module.declare_function("js_ioredis_incr", I64, &[I64, I64]);
    module.declare_function("js_ioredis_new", I64, &[I64]);
    module.declare_function("js_ioredis_ping", I64, &[I64]);
    module.declare_function("js_ioredis_quit", I64, &[I64]);
    module.declare_function("js_ioredis_set", I64, &[I64, I64, I64]);
    module.declare_function("js_ioredis_setex", I64, &[I64, I64, DOUBLE, I64]);

    // ========== MongoDB ==========
    module.declare_function("js_mongodb_client_close", I64, &[I64]);
    module.declare_function("js_mongodb_client_connect", I64, &[I64]);
    module.declare_function("js_mongodb_client_db", I64, &[I64, I64]);
    module.declare_function("js_mongodb_client_list_databases", I64, &[I64]);
    module.declare_function("js_mongodb_client_new", I64, &[I64]);
    // _value wrappers (JSON-stringify f64 JSValue arg, forward to existing fns)
    module.declare_function("js_mongodb_collection_count_value", I64, &[I64, DOUBLE]);
    module.declare_function("js_mongodb_collection_delete_many_value", I64, &[I64, DOUBLE]);
    module.declare_function("js_mongodb_collection_delete_one_value", I64, &[I64, DOUBLE]);
    module.declare_function("js_mongodb_collection_find_one_value", I64, &[I64, DOUBLE]);
    module.declare_function("js_mongodb_collection_find_value", I64, &[I64, DOUBLE]);
    module.declare_function("js_mongodb_collection_insert_many_value", I64, &[I64, DOUBLE]);
    module.declare_function("js_mongodb_collection_insert_one_value", I64, &[I64, DOUBLE]);
    module.declare_function("js_mongodb_collection_update_many_value", I64, &[I64, DOUBLE, DOUBLE]);
    module.declare_function("js_mongodb_collection_update_one_value", I64, &[I64, DOUBLE, DOUBLE]);
    module.declare_function("js_mongodb_collection_count", I64, &[I64, I64]);
    module.declare_function("js_mongodb_collection_delete_many", I64, &[I64, I64]);
    module.declare_function("js_mongodb_collection_delete_one", I64, &[I64, I64]);
    module.declare_function("js_mongodb_collection_find", I64, &[I64, I64]);
    module.declare_function("js_mongodb_collection_find_one", I64, &[I64, I64]);
    module.declare_function("js_mongodb_collection_insert_many", I64, &[I64, I64]);
    module.declare_function("js_mongodb_collection_insert_one", I64, &[I64, I64]);
    module.declare_function("js_mongodb_collection_update_many", I64, &[I64, I64, I64]);
    module.declare_function("js_mongodb_collection_update_one", I64, &[I64, I64, I64]);
    module.declare_function("js_mongodb_connect", I64, &[I64]);
    module.declare_function("js_mongodb_db_collection", I64, &[I64, I64]);
    module.declare_function("js_mongodb_db_list_collections", I64, &[I64]);

    // ========== bcrypt / argon2 ==========
    module.declare_function("js_argon2_hash", I64, &[I64]);
    module.declare_function("js_argon2_hash_options", I64, &[I64, I64]);
    module.declare_function("js_argon2_verify", I64, &[I64, I64]);
    module.declare_function("js_bcrypt_compare", I64, &[I64, I64]);
    module.declare_function("js_bcrypt_compare_sync", DOUBLE, &[I64, I64]);
    module.declare_function("js_bcrypt_gen_salt", I64, &[DOUBLE]);
    module.declare_function("js_bcrypt_hash", I64, &[I64, DOUBLE]);
    module.declare_function("js_bcrypt_hash_sync", I64, &[I64, DOUBLE]);

    // ========== perry/thread (parallelMap, parallelFilter, spawn) ==========
    module.declare_function("js_thread_parallel_map", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_thread_parallel_filter", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_thread_spawn", DOUBLE, &[DOUBLE]);

    // ========== jsonwebtoken / JWT ==========
    module.declare_function("js_jwt_decode", I64, &[I64]);
    module.declare_function("js_jwt_sign", I64, &[I64, I64, DOUBLE, I64]);
    module.declare_function("js_jwt_sign_es256", I64, &[I64, I64, DOUBLE, I64]);
    module.declare_function("js_jwt_sign_rs256", I64, &[I64, I64, DOUBLE, I64]);
    module.declare_function("js_jwt_verify", I64, &[I64, I64]);

    // ========== axios / node-fetch ==========
    module.declare_function("js_axios_create", DOUBLE, &[I64]);
    module.declare_function("js_axios_delete", I64, &[I64]);
    module.declare_function("js_axios_get", I64, &[I64]);
    module.declare_function("js_axios_post", I64, &[I64, I64]);
    module.declare_function("js_axios_put", I64, &[I64, I64]);
    module.declare_function("js_axios_patch", I64, &[I64, I64]);
    module.declare_function("js_axios_request", I64, &[I64]);
    module.declare_function("js_axios_response_status", DOUBLE, &[I64]);
    module.declare_function("js_axios_response_status_text", I64, &[I64]);
    module.declare_function("js_axios_response_data", I64, &[I64]);

    // ========== sharp / image ==========
    module.declare_function("js_sharp_blur", I64, &[I64, DOUBLE]);
    module.declare_function("js_sharp_flip", I64, &[I64]);
    module.declare_function("js_sharp_flop", I64, &[I64]);
    module.declare_function("js_sharp_from_buffer", I64, &[I64, DOUBLE]);
    module.declare_function("js_sharp_from_file", I64, &[I64]);
    module.declare_function("js_sharp_grayscale", I64, &[I64]);
    module.declare_function("js_sharp_metadata", I64, &[I64]);
    module.declare_function("js_sharp_negate", I64, &[I64]);
    module.declare_function("js_sharp_quality", I64, &[I64, DOUBLE]);
    module.declare_function("js_sharp_resize", I64, &[I64, DOUBLE, DOUBLE]);
    module.declare_function("js_sharp_rotate", I64, &[I64, DOUBLE]);
    module.declare_function("js_sharp_to_buffer", I64, &[I64]);
    module.declare_function("js_sharp_to_file", I64, &[I64, I64]);
    module.declare_function("js_sharp_to_format", I64, &[I64, I64]);

    // ========== cron / scheduler ==========
    module.declare_function("js_cron_clear_interval", VOID, &[I64]);
    module.declare_function("js_cron_clear_timeout", VOID, &[I64]);
    module.declare_function("js_cron_describe", I64, &[I64]);
    module.declare_function("js_cron_job_is_running", DOUBLE, &[I64]);
    module.declare_function("js_cron_job_start", VOID, &[I64]);
    module.declare_function("js_cron_job_stop", VOID, &[I64]);
    module.declare_function("js_cron_next_date", I64, &[I64]);
    module.declare_function("js_cron_next_dates", I64, &[I64, DOUBLE]);
    module.declare_function("js_cron_schedule", I64, &[I64, I64]);
    module.declare_function("js_cron_set_interval", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_cron_set_timeout", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_cron_timer_has_pending", I32, &[]);
    module.declare_function("js_cron_timer_tick", I32, &[]);
    module.declare_function("js_cron_validate", DOUBLE, &[I64]);

    // ========== async_hooks / AsyncLocalStorage ==========
    module.declare_function("js_async_local_storage_disable", VOID, &[I64]);
    module.declare_function("js_async_local_storage_enter_with", VOID, &[I64, DOUBLE]);
    module.declare_function("js_async_local_storage_exit", DOUBLE, &[I64, I64]);
    module.declare_function("js_async_local_storage_get_store", DOUBLE, &[I64]);
    module.declare_function("js_async_local_storage_new", I64, &[]);
    module.declare_function("js_async_local_storage_run", DOUBLE, &[I64, DOUBLE, I64]);

    // ========== zlib ==========
    module.declare_function("js_zlib_deflate_sync", I64, &[I64]);
    module.declare_function("js_zlib_gunzip", I64, &[I64]);
    module.declare_function("js_zlib_gunzip_sync", I64, &[I64]);
    module.declare_function("js_zlib_gzip", I64, &[I64]);
    module.declare_function("js_zlib_gzip_sync", I64, &[I64]);
    module.declare_function("js_zlib_inflate_sync", I64, &[I64]);

    // ========== Buffer ==========
    module.declare_function("js_buffer_alloc_unsafe", I64, &[I32]);
    module.declare_function("js_buffer_byte_length", I32, &[I64]);
    module.declare_function("js_buffer_concat", I64, &[I64]);
    module.declare_function("js_buffer_copy", I32, &[I64, I64, I32, I32, I32]);
    module.declare_function("js_buffer_equals", I32, &[I64, I64]);
    module.declare_function("js_buffer_fill", I64, &[I64, I32]);
    module.declare_function("js_buffer_from_value", I64, &[I64, I32]);
    module.declare_function("js_buffer_is_buffer", I32, &[I64]);
    module.declare_function("js_buffer_print", VOID, &[I64]);
    module.declare_function("js_buffer_set", VOID, &[I64, I32, I32]);
    module.declare_function("js_buffer_set_from", VOID, &[I64, I64, I32]);
    module.declare_function("js_buffer_slice", I64, &[I64, I32, I32]);
    module.declare_function("js_buffer_to_string", I64, &[I64, I32]);
    module.declare_function("js_buffer_write", I32, &[I64, I64, I32, I32]);

    // ========== child_process ==========
    module.declare_function("js_child_process_exec_sync", I64, &[I64, I64]);
    module.declare_function("js_child_process_get_process_status", I64, &[DOUBLE]);
    module.declare_function("js_child_process_kill_process", I32, &[DOUBLE]);
    module.declare_function("js_child_process_spawn_background", I64, &[DOUBLE, I64, DOUBLE, DOUBLE]);
    module.declare_function("js_child_process_spawn_sync", I64, &[I64, I64, I64]);

    // ========== cheerio ==========
    module.declare_function("js_cheerio_load", I64, &[I64]);
    module.declare_function("js_cheerio_load_fragment", I64, &[I64]);
    module.declare_function("js_cheerio_select", I64, &[I64, I64]);
    module.declare_function("js_cheerio_selection_attr", I64, &[I64, I64]);
    module.declare_function("js_cheerio_selection_attrs", I64, &[I64, I64]);
    module.declare_function("js_cheerio_selection_children", I64, &[I64, I64]);
    module.declare_function("js_cheerio_selection_eq", I64, &[I64, DOUBLE]);
    module.declare_function("js_cheerio_selection_find", I64, &[I64, I64]);
    module.declare_function("js_cheerio_selection_first", I64, &[I64]);
    module.declare_function("js_cheerio_selection_has_class", DOUBLE, &[I64, I64]);
    module.declare_function("js_cheerio_selection_html", I64, &[I64]);
    module.declare_function("js_cheerio_selection_is", DOUBLE, &[I64, I64]);
    module.declare_function("js_cheerio_selection_last", I64, &[I64]);
    module.declare_function("js_cheerio_selection_length", DOUBLE, &[I64]);
    module.declare_function("js_cheerio_selection_parent", I64, &[I64]);
    module.declare_function("js_cheerio_selection_text", I64, &[I64]);
    module.declare_function("js_cheerio_selection_texts", I64, &[I64]);
    module.declare_function("js_cheerio_selection_to_array", I64, &[I64]);

    // ========== URL / URLSearchParams ==========
    // Rust runtime signatures (see crates/perry-runtime/src/url.rs):
    //   js_url_new(*mut StringHeader)                         -> *mut ObjectHeader
    //   js_url_new_with_base(*mut StringHeader, *mut ...)     -> *mut ObjectHeader
    //   js_url_get_{href,pathname,protocol,host,hostname,port,search,hash,origin,search_params}
    //     (*mut ObjectHeader)                                  -> f64 (NaN-boxed string)
    //   js_url_search_params_new(*mut StringHeader)            -> *mut ObjectHeader
    //   js_url_search_params_new_empty()                       -> *mut ObjectHeader
    //   js_url_search_params_get(*mut ObjectHeader, *mut StringHeader)
    //                                                          -> *mut StringHeader (null if missing)
    //   js_url_search_params_has(*mut ObjectHeader, *mut StringHeader)
    //                                                          -> f64 (0.0 or 1.0)
    //   js_url_search_params_set/append(*mut ObjectHeader, *mut ..., *mut ...) -> void
    //   js_url_search_params_delete(*mut ObjectHeader, *mut StringHeader)      -> void
    //   js_url_search_params_to_string(*mut ObjectHeader)     -> *mut StringHeader
    //   js_url_search_params_get_all(*mut ObjectHeader, *mut StringHeader)
    //                                                          -> f64 (NaN-boxed array)
    module.declare_function("js_url_file_url_to_path", DOUBLE, &[DOUBLE]);
    module.declare_function("js_url_get_hash", DOUBLE, &[I64]);
    module.declare_function("js_url_get_host", DOUBLE, &[I64]);
    module.declare_function("js_url_get_hostname", DOUBLE, &[I64]);
    module.declare_function("js_url_get_href", DOUBLE, &[I64]);
    module.declare_function("js_url_get_origin", DOUBLE, &[I64]);
    module.declare_function("js_url_get_pathname", DOUBLE, &[I64]);
    module.declare_function("js_url_get_port", DOUBLE, &[I64]);
    module.declare_function("js_url_get_protocol", DOUBLE, &[I64]);
    module.declare_function("js_url_get_search", DOUBLE, &[I64]);
    module.declare_function("js_url_get_search_params", DOUBLE, &[I64]);
    module.declare_function("js_url_new", I64, &[I64]);
    module.declare_function("js_url_new_with_base", I64, &[I64, I64]);
    module.declare_function("js_url_search_params_append", VOID, &[I64, I64, I64]);
    module.declare_function("js_url_search_params_delete", VOID, &[I64, I64]);
    module.declare_function("js_url_search_params_get", I64, &[I64, I64]);
    module.declare_function("js_url_search_params_get_all", DOUBLE, &[I64, I64]);
    module.declare_function("js_url_search_params_has", DOUBLE, &[I64, I64]);
    module.declare_function("js_url_search_params_new", I64, &[I64]);
    module.declare_function("js_url_search_params_new_empty", I64, &[]);
    module.declare_function("js_url_search_params_set", VOID, &[I64, I64, I64]);
    module.declare_function("js_url_search_params_to_string", I64, &[I64]);

    // ========== WebSocket ==========
    module.declare_function("js_ws_close", VOID, &[I64]);
    module.declare_function("js_ws_connect", I64, &[I64]);
    module.declare_function("js_ws_connect_start", DOUBLE, &[DOUBLE]);
    module.declare_function("js_ws_handle_to_i64", I64, &[DOUBLE]);
    module.declare_function("js_ws_is_open", DOUBLE, &[I64]);
    module.declare_function("js_ws_message_count", DOUBLE, &[I64]);
    module.declare_function("js_ws_on", I64, &[I64, I64, I64]);
    module.declare_function("js_ws_receive", I64, &[I64]);
    module.declare_function("js_ws_send", VOID, &[I64, I64]);
    module.declare_function("js_ws_server_close", VOID, &[I64]);
    module.declare_function("js_ws_server_new", I64, &[DOUBLE]);
    module.declare_function("js_ws_wait_for_message", I64, &[I64, DOUBLE]);

    // ========== SQLite ==========
    module.declare_function("js_sqlite_close", VOID, &[I64]);
    module.declare_function("js_sqlite_exec", VOID, &[I64, I64]);
    module.declare_function("js_sqlite_open", I64, &[I64]);
    module.declare_function("js_sqlite_pragma", I64, &[I64, I64, I64]);
    module.declare_function("js_sqlite_prepare", I64, &[I64, I64]);
    module.declare_function("js_sqlite_stmt_all", I64, &[I64, I64]);
    module.declare_function("js_sqlite_stmt_get", I64, &[I64, I64]);
    module.declare_function("js_sqlite_stmt_run", I64, &[I64, I64]);
    module.declare_function("js_sqlite_transaction", I64, &[I64, I64]);
    module.declare_function("js_sqlite_transaction_commit", VOID, &[I64]);
    module.declare_function("js_sqlite_transaction_rollback", VOID, &[I64]);

    // ========== OS ==========
    module.declare_function("js_os_cpus", I64, &[]);
    module.declare_function("js_os_freemem", DOUBLE, &[]);
    module.declare_function("js_os_homedir", I64, &[]);
    module.declare_function("js_os_network_interfaces", I64, &[]);
    module.declare_function("js_os_tmpdir", I64, &[]);
    module.declare_function("js_os_totalmem", DOUBLE, &[]);
    module.declare_function("js_os_uptime", DOUBLE, &[]);
    module.declare_function("js_os_user_info", I64, &[]);

    // ========== Crypto ==========
    module.declare_function("js_crypto_aes256_decrypt", I64, &[I64, I64, I64]);
    module.declare_function("js_crypto_aes256_encrypt", I64, &[I64, I64, I64]);
    module.declare_function("js_crypto_aes256_gcm_decrypt", I64, &[I64, I64, I64]);
    module.declare_function("js_crypto_aes256_gcm_encrypt", I64, &[I64, I64, I64]);
    module.declare_function("js_crypto_hkdf_sha256", I64, &[I64, I64, I64, DOUBLE]);
    module.declare_function("js_crypto_pbkdf2", I64, &[I64, I64, DOUBLE, DOUBLE]);
    module.declare_function("js_crypto_random_bytes_hex", I64, &[DOUBLE]);
    module.declare_function("js_crypto_random_nonce", I64, &[]);
    module.declare_function("js_crypto_scrypt", I64, &[I64, I64, DOUBLE]);
    module.declare_function("js_crypto_scrypt_custom", I64, &[I64, I64, DOUBLE, DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_crypto_x25519_keypair", I64, &[]);
    module.declare_function("js_crypto_x25519_shared_secret", I64, &[I64, I64]);
    module.declare_function("js_keccak256_native", I64, &[I64]);
    module.declare_function("js_keccak256_native_bytes", I64, &[I64]);

    // ========== Nanoid ==========
    module.declare_function("js_nanoid", I64, &[DOUBLE]);
    module.declare_function("js_nanoid_custom", I64, &[I64, DOUBLE]);

    // ========== Commander CLI ==========
    module.declare_function("js_commander_action", I64, &[I64, I64]);
    module.declare_function("js_commander_command", I64, &[I64, I64]);
    module.declare_function("js_commander_description", I64, &[I64, I64]);
    module.declare_function("js_commander_get_option", I64, &[I64, I64]);
    module.declare_function("js_commander_get_option_bool", DOUBLE, &[I64, I64]);
    module.declare_function("js_commander_get_option_number", DOUBLE, &[I64, I64]);
    module.declare_function("js_commander_name", I64, &[I64, I64]);
    module.declare_function("js_commander_new", I64, &[]);
    module.declare_function("js_commander_option", I64, &[I64, I64, I64, I64]);
    module.declare_function("js_commander_opts", I64, &[I64]);
    module.declare_function("js_commander_parse", I64, &[I64, DOUBLE]);
    module.declare_function("js_commander_required_option", I64, &[I64, I64, I64, I64]);
    module.declare_function("js_commander_version", I64, &[I64, I64]);

    // ========== Dotenv ==========
    module.declare_function("js_dotenv_config", DOUBLE, &[]);
    module.declare_function("js_dotenv_config_path", DOUBLE, &[I64]);
    module.declare_function("js_dotenv_parse", I64, &[I64]);

    // ========== Date libs (dayjs/datefns/moment) ==========
    module.declare_function("js_datefns_add_days", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_datefns_add_months", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_datefns_add_years", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_datefns_difference_in_days", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_datefns_difference_in_hours", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_datefns_difference_in_minutes", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_datefns_end_of_day", DOUBLE, &[DOUBLE]);
    module.declare_function("js_datefns_format", I64, &[DOUBLE, I64]);
    module.declare_function("js_datefns_is_after", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_datefns_is_before", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_datefns_parse_iso", DOUBLE, &[I64]);
    module.declare_function("js_datefns_start_of_day", DOUBLE, &[DOUBLE]);
    module.declare_function("js_dayjs_add", DOUBLE, &[I64, DOUBLE, I64]);
    module.declare_function("js_dayjs_date", DOUBLE, &[I64]);
    module.declare_function("js_dayjs_day", DOUBLE, &[I64]);
    module.declare_function("js_dayjs_diff", DOUBLE, &[I64, I64, I64]);
    module.declare_function("js_dayjs_end_of", DOUBLE, &[I64, I64]);
    module.declare_function("js_dayjs_format", I64, &[I64, I64]);
    module.declare_function("js_dayjs_from_timestamp", DOUBLE, &[DOUBLE]);
    module.declare_function("js_dayjs_hour", DOUBLE, &[I64]);
    module.declare_function("js_dayjs_is_after", DOUBLE, &[I64, I64]);
    module.declare_function("js_dayjs_is_before", DOUBLE, &[I64, I64]);
    module.declare_function("js_dayjs_is_same", DOUBLE, &[I64, I64]);
    module.declare_function("js_dayjs_is_valid", DOUBLE, &[I64]);
    module.declare_function("js_dayjs_millisecond", DOUBLE, &[I64]);
    module.declare_function("js_dayjs_minute", DOUBLE, &[I64]);
    module.declare_function("js_dayjs_month", DOUBLE, &[I64]);
    module.declare_function("js_dayjs_now", DOUBLE, &[]);
    module.declare_function("js_dayjs_parse", DOUBLE, &[I64]);
    module.declare_function("js_dayjs_second", DOUBLE, &[I64]);
    module.declare_function("js_dayjs_start_of", DOUBLE, &[I64, I64]);
    module.declare_function("js_dayjs_subtract", DOUBLE, &[I64, DOUBLE, I64]);
    module.declare_function("js_dayjs_to_iso_string", I64, &[I64]);
    module.declare_function("js_dayjs_unix", DOUBLE, &[I64]);
    module.declare_function("js_dayjs_value_of", DOUBLE, &[I64]);
    module.declare_function("js_dayjs_year", DOUBLE, &[I64]);
    module.declare_function("js_moment_add", I64, &[I64, DOUBLE, I64]);
    module.declare_function("js_moment_date", DOUBLE, &[I64]);
    module.declare_function("js_moment_day", DOUBLE, &[I64]);
    module.declare_function("js_moment_diff", DOUBLE, &[I64, I64, I64]);
    module.declare_function("js_moment_end_of", I64, &[I64, I64]);
    module.declare_function("js_moment_format", I64, &[I64, I64]);
    module.declare_function("js_moment_from_timestamp", I64, &[DOUBLE]);
    module.declare_function("js_moment_hour", DOUBLE, &[I64]);
    module.declare_function("js_moment_is_valid", DOUBLE, &[I64]);
    module.declare_function("js_moment_millisecond", DOUBLE, &[I64]);
    module.declare_function("js_moment_minute", DOUBLE, &[I64]);
    module.declare_function("js_moment_month", DOUBLE, &[I64]);
    module.declare_function("js_moment_now", I64, &[]);
    module.declare_function("js_moment_parse", I64, &[I64]);
    module.declare_function("js_moment_second", DOUBLE, &[I64]);
    module.declare_function("js_moment_start_of", I64, &[I64, I64]);
    module.declare_function("js_moment_subtract", I64, &[I64, DOUBLE, I64]);
    module.declare_function("js_moment_unix", DOUBLE, &[I64]);
    module.declare_function("js_moment_value_of", DOUBLE, &[I64]);
    module.declare_function("js_moment_year", DOUBLE, &[I64]);

    // ========== Decimal.js ==========
    module.declare_function("js_decimal_abs", I64, &[I64]);
    module.declare_function("js_decimal_ceil", I64, &[I64]);
    module.declare_function("js_decimal_cmp", DOUBLE, &[I64, I64]);
    module.declare_function("js_decimal_cmp_value", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_decimal_coerce_to_handle", I64, &[DOUBLE]);
    module.declare_function("js_decimal_div", I64, &[I64, I64]);
    module.declare_function("js_decimal_div_number", I64, &[I64, DOUBLE]);
    module.declare_function("js_decimal_div_value", I64, &[I64, DOUBLE]);
    module.declare_function("js_decimal_eq", DOUBLE, &[I64, I64]);
    module.declare_function("js_decimal_eq_value", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_decimal_floor", I64, &[I64]);
    module.declare_function("js_decimal_from_number", I64, &[DOUBLE]);
    module.declare_function("js_decimal_from_string", I64, &[I64]);
    module.declare_function("js_decimal_gt", DOUBLE, &[I64, I64]);
    module.declare_function("js_decimal_gt_value", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_decimal_gte", DOUBLE, &[I64, I64]);
    module.declare_function("js_decimal_gte_value", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_decimal_is_negative", DOUBLE, &[I64]);
    module.declare_function("js_decimal_is_positive", DOUBLE, &[I64]);
    module.declare_function("js_decimal_is_zero", DOUBLE, &[I64]);
    module.declare_function("js_decimal_lt", DOUBLE, &[I64, I64]);
    module.declare_function("js_decimal_lt_value", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_decimal_lte", DOUBLE, &[I64, I64]);
    module.declare_function("js_decimal_lte_value", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_decimal_minus", I64, &[I64, I64]);
    module.declare_function("js_decimal_minus_number", I64, &[I64, DOUBLE]);
    module.declare_function("js_decimal_minus_value", I64, &[I64, DOUBLE]);
    module.declare_function("js_decimal_mod", I64, &[I64, I64]);
    module.declare_function("js_decimal_mod_value", I64, &[I64, DOUBLE]);
    module.declare_function("js_decimal_neg", I64, &[I64]);
    module.declare_function("js_decimal_plus", I64, &[I64, I64]);
    module.declare_function("js_decimal_plus_number", I64, &[I64, DOUBLE]);
    module.declare_function("js_decimal_plus_value", I64, &[I64, DOUBLE]);
    module.declare_function("js_decimal_pow", I64, &[I64, DOUBLE]);
    module.declare_function("js_decimal_round", I64, &[I64]);
    module.declare_function("js_decimal_sqrt", I64, &[I64]);
    module.declare_function("js_decimal_times", I64, &[I64, I64]);
    module.declare_function("js_decimal_times_number", I64, &[I64, DOUBLE]);
    module.declare_function("js_decimal_times_value", I64, &[I64, DOUBLE]);
    module.declare_function("js_decimal_to_fixed", I64, &[I64, DOUBLE]);
    module.declare_function("js_decimal_to_number", DOUBLE, &[I64]);
    module.declare_function("js_decimal_to_string", I64, &[I64]);

    // ========== Ethers / blockchain ==========
    module.declare_function("js_ethers_format_ether", I64, &[I64]);
    module.declare_function("js_ethers_format_units", I64, &[I64, DOUBLE]);
    module.declare_function("js_ethers_get_address", I64, &[I64]);
    module.declare_function("js_ethers_parse_ether", I64, &[I64]);
    module.declare_function("js_ethers_parse_units", I64, &[I64, DOUBLE]);

    // ========== Lodash ==========
    module.declare_function("js_lodash_camel_case", I64, &[I64]);
    module.declare_function("js_lodash_capitalize", I64, &[I64]);
    module.declare_function("js_lodash_chunk", I64, &[I64, DOUBLE]);
    module.declare_function("js_lodash_clamp", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_lodash_compact", I64, &[I64]);
    module.declare_function("js_lodash_concat", I64, &[I64, I64]);
    module.declare_function("js_lodash_difference", I64, &[I64, I64]);
    module.declare_function("js_lodash_drop", I64, &[I64, DOUBLE]);
    module.declare_function("js_lodash_drop_right", I64, &[I64, DOUBLE]);
    module.declare_function("js_lodash_ends_with", DOUBLE, &[I64, I64]);
    module.declare_function("js_lodash_escape", I64, &[I64]);
    module.declare_function("js_lodash_first", DOUBLE, &[I64]);
    module.declare_function("js_lodash_flatten", I64, &[I64]);
    module.declare_function("js_lodash_in_range", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_lodash_includes", DOUBLE, &[I64, I64]);
    module.declare_function("js_lodash_initial", I64, &[I64]);
    module.declare_function("js_lodash_kebab_case", I64, &[I64]);
    module.declare_function("js_lodash_last", DOUBLE, &[I64]);
    module.declare_function("js_lodash_lower_case", I64, &[I64]);
    module.declare_function("js_lodash_lower_first", I64, &[I64]);
    module.declare_function("js_lodash_pad", I64, &[I64, DOUBLE]);
    module.declare_function("js_lodash_pad_end", I64, &[I64, DOUBLE]);
    module.declare_function("js_lodash_pad_start", I64, &[I64, DOUBLE]);
    module.declare_function("js_lodash_random", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_lodash_repeat", I64, &[I64, DOUBLE]);
    module.declare_function("js_lodash_replace", I64, &[I64, I64, I64]);
    module.declare_function("js_lodash_reverse", I64, &[I64]);
    module.declare_function("js_lodash_size", DOUBLE, &[I64]);
    module.declare_function("js_lodash_snake_case", I64, &[I64]);
    module.declare_function("js_lodash_split", I64, &[I64, I64]);
    module.declare_function("js_lodash_start_case", I64, &[I64]);
    module.declare_function("js_lodash_starts_with", DOUBLE, &[I64, I64]);
    module.declare_function("js_lodash_tail", I64, &[I64]);
    module.declare_function("js_lodash_take", I64, &[I64, DOUBLE]);
    module.declare_function("js_lodash_take_right", I64, &[I64, DOUBLE]);
    module.declare_function("js_lodash_trim", I64, &[I64]);
    module.declare_function("js_lodash_trim_end", I64, &[I64]);
    module.declare_function("js_lodash_trim_start", I64, &[I64]);
    module.declare_function("js_lodash_truncate", I64, &[I64, DOUBLE]);
    module.declare_function("js_lodash_unescape", I64, &[I64]);
    module.declare_function("js_lodash_uniq", I64, &[I64]);
    module.declare_function("js_lodash_upper_case", I64, &[I64]);
    module.declare_function("js_lodash_upper_first", I64, &[I64]);

    // ========== LRU Cache ==========
    module.declare_function("js_lru_cache_clear", VOID, &[I64]);
    module.declare_function("js_lru_cache_delete", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_lru_cache_get", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_lru_cache_has", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_lru_cache_new", I64, &[DOUBLE]);
    module.declare_function("js_lru_cache_peek", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_lru_cache_set", I64, &[I64, DOUBLE, DOUBLE]);
    module.declare_function("js_lru_cache_size", DOUBLE, &[I64]);

    // ========== Event emitter ==========
    module.declare_function("js_event_emitter_emit", DOUBLE, &[I64, I64, DOUBLE]);
    module.declare_function("js_event_emitter_emit0", DOUBLE, &[I64, I64]);
    module.declare_function("js_event_emitter_listener_count", DOUBLE, &[I64, I64]);
    module.declare_function("js_event_emitter_new", I64, &[]);
    module.declare_function("js_event_emitter_on", I64, &[I64, I64, I64]);
    module.declare_function("js_event_emitter_remove_all_listeners", I64, &[I64, I64]);
    module.declare_function("js_event_emitter_remove_listener", I64, &[I64, I64, I64]);

    // ========== Fastify ==========
    module.declare_function("js_fastify_add_hook", I32, &[I64, I64, I64]);
    module.declare_function("js_fastify_all", I32, &[I64, I64, I64]);
    module.declare_function("js_fastify_create", I64, &[]);
    module.declare_function("js_fastify_create_with_opts", I64, &[DOUBLE]);
    module.declare_function("js_fastify_ctx_html", DOUBLE, &[I64, I64, DOUBLE]);
    module.declare_function("js_fastify_ctx_json", DOUBLE, &[I64, DOUBLE, DOUBLE]);
    module.declare_function("js_fastify_ctx_redirect", DOUBLE, &[I64, I64, DOUBLE]);
    module.declare_function("js_fastify_ctx_text", DOUBLE, &[I64, I64, DOUBLE]);
    module.declare_function("js_fastify_delete", I32, &[I64, I64, I64]);
    module.declare_function("js_fastify_get", I32, &[I64, I64, I64]);
    module.declare_function("js_fastify_head", I32, &[I64, I64, I64]);
    module.declare_function("js_fastify_listen", VOID, &[I64, DOUBLE, I64]);
    module.declare_function("js_fastify_options", I32, &[I64, I64, I64]);
    module.declare_function("js_fastify_patch", I32, &[I64, I64, I64]);
    module.declare_function("js_fastify_post", I32, &[I64, I64, I64]);
    module.declare_function("js_fastify_put", I32, &[I64, I64, I64]);
    module.declare_function("js_fastify_register", I32, &[I64, I64, DOUBLE]);
    module.declare_function("js_fastify_reply_header", I64, &[I64, I64, I64]);
    module.declare_function("js_fastify_reply_send", I32, &[I64, DOUBLE]);
    module.declare_function("js_fastify_reply_status", I64, &[I64, DOUBLE]);
    module.declare_function("js_fastify_req_body", I64, &[I64]);
    module.declare_function("js_fastify_req_get_user_data", DOUBLE, &[I64]);
    module.declare_function("js_fastify_req_header", I64, &[I64, I64]);
    module.declare_function("js_fastify_req_headers", I64, &[I64]);
    module.declare_function("js_fastify_req_json", DOUBLE, &[I64]);
    module.declare_function("js_fastify_req_method", I64, &[I64]);
    module.declare_function("js_fastify_req_param", I64, &[I64, I64]);
    module.declare_function("js_fastify_req_params", I64, &[I64]);
    module.declare_function("js_fastify_req_query", I64, &[I64]);
    module.declare_function("js_fastify_req_query_object", DOUBLE, &[I64]);
    module.declare_function("js_fastify_req_set_user_data", VOID, &[I64, DOUBLE]);
    module.declare_function("js_fastify_req_url", I64, &[I64]);
    module.declare_function("js_fastify_route", I32, &[I64, I64, I64, I64]);
    module.declare_function("js_fastify_set_error_handler", I32, &[I64, I64]);

    // ========== Nodemailer ==========
    module.declare_function("js_nodemailer_create_transport", DOUBLE, &[I64]);
    module.declare_function("js_nodemailer_send_mail", I64, &[I64, I64]);
    module.declare_function("js_nodemailer_verify", I64, &[I64]);

    // ========== Rate limit ==========
    module.declare_function("js_ratelimit_block", I64, &[I64, I64, DOUBLE]);
    module.declare_function("js_ratelimit_consume", I64, &[I64, I64, DOUBLE]);
    module.declare_function("js_ratelimit_create", I64, &[I64]);
    module.declare_function("js_ratelimit_delete", I64, &[I64, I64]);
    module.declare_function("js_ratelimit_get", I64, &[I64, I64]);
    module.declare_function("js_ratelimit_penalty", I64, &[I64, I64, DOUBLE]);
    module.declare_function("js_ratelimit_reward", I64, &[I64, I64, DOUBLE]);

    // ========== Validator ==========
    module.declare_function("js_validator_contains", DOUBLE, &[I64, I64]);
    module.declare_function("js_validator_equals", DOUBLE, &[I64, I64]);
    module.declare_function("js_validator_is_alpha", DOUBLE, &[I64]);
    module.declare_function("js_validator_is_alphanumeric", DOUBLE, &[I64]);
    module.declare_function("js_validator_is_email", DOUBLE, &[I64]);
    module.declare_function("js_validator_is_empty", DOUBLE, &[I64]);
    module.declare_function("js_validator_is_float", DOUBLE, &[I64]);
    module.declare_function("js_validator_is_hexadecimal", DOUBLE, &[I64]);
    module.declare_function("js_validator_is_int", DOUBLE, &[I64]);
    module.declare_function("js_validator_is_json", DOUBLE, &[I64]);
    module.declare_function("js_validator_is_length", DOUBLE, &[I64, DOUBLE, DOUBLE]);
    module.declare_function("js_validator_is_lowercase", DOUBLE, &[I64]);
    module.declare_function("js_validator_is_numeric", DOUBLE, &[I64]);
    module.declare_function("js_validator_is_uppercase", DOUBLE, &[I64]);
    module.declare_function("js_validator_is_url", DOUBLE, &[I64]);
    module.declare_function("js_validator_is_uuid", DOUBLE, &[I64]);

    // ========== Date ==========
    module.declare_function("js_date_to_locale_string", I64, &[DOUBLE]);

    // ========== String ==========
    module.declare_function("js_string_split_regex", I64, &[I64, I64]);

    // ========== Object ==========
    module.declare_function("js_object_delete_dynamic", I32, &[I64, DOUBLE]);
    module.declare_function("js_object_get_prototype_of", DOUBLE, &[DOUBLE]);

    // ========== Math ==========
    module.declare_function("js_math_acos", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_asin", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_atan", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_atan2", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_math_cos", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_expm1", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_log", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_log10", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_log1p", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_log2", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_sin", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_tan", DOUBLE, &[DOUBLE]);

    // ========== Number ==========
    module.declare_function("js_number_is_finite", DOUBLE, &[DOUBLE]);

    // ========== JSON ==========
    module.declare_function("js_json_get_bool", DOUBLE, &[I64, I64]);
    module.declare_function("js_json_get_number", DOUBLE, &[I64, I64]);
    module.declare_function("js_json_get_string", I64, &[I64, I64]);
    module.declare_function("js_json_is_valid", DOUBLE, &[I64]);
    module.declare_function("js_json_stringify_bool", I64, &[DOUBLE]);
    module.declare_function("js_json_stringify_null", I64, &[]);
    module.declare_function("js_json_stringify_number", I64, &[DOUBLE]);
    module.declare_function("js_json_stringify_string", I64, &[I64]);

    // ========== Map / Set / WeakMap ==========
    module.declare_function("js_set_property", VOID, &[DOUBLE, I64, I64, DOUBLE]);

    // ========== Error ==========
    module.declare_function("js_error_get_message", I64, &[I64]);

    // ========== Promise ==========
    module.declare_function("js_await_js_promise", DOUBLE, &[DOUBLE]);

    // ========== Text encoding ==========
    module.declare_function("js_text_decoder_decode", I64, &[I64]);
    module.declare_function("js_text_encoder_encode", I64, &[DOUBLE]);

    // ========== Closures / functions ==========
    module.declare_function("js_call_function", DOUBLE, &[I64, I64, I64, I64, I64]);
    module.declare_function("js_create_callback", DOUBLE, &[I64, I64, I64]);

    // ========== NaN-boxing / typeof / is_* ==========
    module.declare_function("js_dynamic_neg", DOUBLE, &[DOUBLE]);
    module.declare_function("js_dynamic_string_equals", I32, &[DOUBLE, DOUBLE]);
    module.declare_function("js_is_nan", DOUBLE, &[DOUBLE]);
    module.declare_function("js_jsvalue_compare", I32, &[DOUBLE, DOUBLE]);
    module.declare_function("js_jsvalue_equals", I32, &[DOUBLE, DOUBLE]);
    module.declare_function("js_jsvalue_loose_equals", I32, &[DOUBLE, DOUBLE]);

    // ========== GC ==========
    module.declare_function("js_gc_collect", VOID, &[]);

    // ========== Console ==========
    module.declare_function("js_console_assert", VOID, &[DOUBLE, I64]);
    module.declare_function("js_console_assert_spread", VOID, &[DOUBLE, I64]);
    module.declare_function("js_console_group", VOID, &[I64]);

    // ========== Fetch ==========
    module.declare_function("js_fetch_get", I64, &[I64]);
    module.declare_function("js_fetch_get_with_auth", I64, &[I64, I64]);
    module.declare_function("js_fetch_post", I64, &[I64, I64, I64]);
    module.declare_function("js_fetch_post_with_auth", I64, &[I64, I64, I64]);
    module.declare_function("js_fetch_stream_close", DOUBLE, &[DOUBLE]);
    module.declare_function("js_fetch_stream_poll", I64, &[DOUBLE]);
    module.declare_function("js_fetch_stream_start", DOUBLE, &[I64, I64, I64, I64]);
    module.declare_function("js_fetch_stream_status", DOUBLE, &[DOUBLE]);
    module.declare_function("js_fetch_text", I64, &[I64]);
    module.declare_function("js_fetch_with_options", I64, &[I64, I64, I64, I64]);

    // ========== Net ==========
    module.declare_function("js_net_create_connection", DOUBLE, &[I32, I64, I64]);
    module.declare_function("js_net_create_server", DOUBLE, &[I64, I64]);

    // ========== Performance ==========
    module.declare_function("js_performance_now", DOUBLE, &[]);

    // ========== Slugify ==========
    module.declare_function("js_slugify", I64, &[I64]);
    module.declare_function("js_slugify_strict", I64, &[I64]);

    // ========== Class registration ==========
    module.declare_function("js_register_class_getter", VOID, &[I64, I64, I64, I64]);
    module.declare_function("js_register_class_method", VOID, &[I64, I64, I64, I64, I64]);

    // ========== Runtime init / module loader ==========
    module.declare_function("js_get_export", DOUBLE, &[I64, I64, I64]);
    module.declare_function("js_get_property", DOUBLE, &[DOUBLE, I64, I64]);
    module.declare_function("js_load_module", I64, &[I64, I64]);
    module.declare_function("js_native_call_method", DOUBLE, &[DOUBLE, I64, I64, I64, I64]);
    module.declare_function("js_native_call_value", DOUBLE, &[DOUBLE, I64, I64]);
    module.declare_function("js_new_from_handle", DOUBLE, &[DOUBLE, I64, I64]);
    module.declare_function("js_new_instance", DOUBLE, &[I64, I64, I64, I64, I64]);
    module.declare_function("js_runtime_init", VOID, &[]);

    // ========== Well-known Symbol conversion hooks ==========
    // Triggered by:
    //   - `js_object_set_symbol_method`: HIR IIFE wrapper for object-literal
    //     computed-key methods whose closure captures `this`
    //     (e.g. `{ [Symbol.toPrimitive](hint) { return this.value; } }`).
    //     Stores the closure AND patches its reserved `this` slot with obj.
    //   - `js_to_primitive`: consulted by `js_number_coerce` and
    //     `js_jsvalue_to_string` to route through a user-defined
    //     `[Symbol.toPrimitive]` method when the value is an object. Called
    //     indirectly from within the runtime; declared here so HIR
    //     `Call(ExternFuncRef("js_to_primitive"), ...)` can also call it.
    //   - `js_register_class_has_instance` / `js_register_class_to_string_tag`:
    //     called from `init_static_fields` for each class whose HIR lowering
    //     lifted a `static [Symbol.hasInstance]()` method or a
    //     `get [Symbol.toStringTag]()` getter to a top-level function with
    //     a `__perry_wk_<hook>_<class>` prefix. The runtime stores the
    //     function pointer against the class_id and consults it from
    //     `js_instanceof` / `js_object_to_string`.
    //   - `js_object_to_string`: implements `Object.prototype.toString.call(x)`
    //     by reading the class's registered `Symbol.toStringTag` getter.
    //     Called directly from HIR via `Call(ExternFuncRef, [obj])`.
    module.declare_function(
        "js_object_set_symbol_method",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function("js_to_primitive", DOUBLE, &[DOUBLE, I32]);
    module.declare_function("js_register_class_has_instance", VOID, &[I32, I64]);
    module.declare_function("js_register_class_to_string_tag", VOID, &[I32, I64]);
    module.declare_function("js_object_to_string", DOUBLE, &[DOUBLE]);

    // ---- Object.groupBy (Node 22+) ----
    // Triggered by HIR variant `Expr::ObjectGroupBy { items, key_fn }`
    // (perry-hir/src/lower.rs catches the AST `Object.groupBy(items, fn)`
    // call site). The runtime implementation walks `items`, invokes
    // `key_fn(item, index)` per element, and materializes a result
    // object grouping items by their string key. See
    // `crates/perry-runtime/src/object.rs::js_object_group_by`.
    //
    // `Array.fromAsync(input)` — Node 22+. Dispatched at the LLVM
    // codegen level in `lower_call.rs` when the receiver is a global
    // and the property is `fromAsync`. The runtime function returns a
    // NaN-boxed Promise pointer; for arrays it forwards to
    // `js_promise_all`, for async iterators it chains `.next()` calls
    // through `array_from_async_step`.
    module.declare_function("js_object_group_by", DOUBLE, &[DOUBLE, I64]);
    module.declare_function("js_array_from_async", DOUBLE, &[DOUBLE]);
}
