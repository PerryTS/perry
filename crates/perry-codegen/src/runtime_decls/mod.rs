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
use crate::types::{DOUBLE, F32, I1, I16, I32, I64, I8, PTR, VOID};

mod arrays;
mod objects;
mod stdlib_ffi;
mod stdlib_ffi_part2;
mod strings;
mod strings_part2;

pub use arrays::declare_phase_b_arrays;
pub use objects::declare_phase_b_objects;
pub use stdlib_ffi::declare_stdlib_ffi;
pub(crate) use stdlib_ffi_part2::declare_stdlib_ffi_part2;
pub use strings::declare_phase_b_strings;
pub(crate) use strings_part2::declare_phase_b_strings_part2;

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
    module.declare_function("js_typed_feedback_maybe_dump_trace", VOID, &[]);
    // Handle-method dispatcher wiring (issue #86). Stdlib provides the
    // real impl; when only runtime is linked, it's a no-op stub.
    module.declare_function("js_stdlib_init_dispatch", VOID, &[]);
    // #1178 — App Group suite-name registration. The CLI bakes
    // `[ios] app_group` from perry.toml into the entry module's `main`
    // prelude as a single call to `perry_app_group_init(ptr, len)` so
    // the iOS/macOS UserDefaults(suiteName:) FFI can resolve it
    // without re-reading the manifest. Declared unconditionally because
    // the runtime always provides the symbol; main only emits the call
    // when `app_metadata.app_group` is `Some`.
    module.declare_function("perry_app_group_init", VOID, &[PTR, I32]);
    // macOS asset-CWD fix: a macOS `.app` launched from Finder starts with
    // CWD=`/`, but the worker bundles assets into `Contents/Resources/`. The
    // `main` prelude calls this unconditionally; the runtime symbol no-ops on
    // non-macOS targets and on binaries that aren't inside an `.app` bundle.
    module.declare_function("perry_macos_bundle_chdir", VOID, &[]);
    // Function-name registry — populated by `main()` once per top-level
    // named function so `console.log(named)` prints `[Function: named]`
    // instead of `[Function (anonymous)]`. See #1202.
    module.declare_function("js_register_function_name", VOID, &[PTR, PTR, I32]);
    // #4101: register a user function's original source text (keyed by the
    // same wrapper/closure address as the name) so `fn.toString()` and
    // `Function.prototype.toString.call(fn)` reconstruct the source.
    module.declare_function("js_register_function_source", VOID, &[PTR, PTR, I32]);

    // Console.
    module.declare_function("js_console_log_dynamic", VOID, &[DOUBLE]);
    module.declare_function("js_console_log_number", VOID, &[DOUBLE]);
    // console.error / console.warn single-arg fast paths (#345). These
    // route the print to stderr / stdout-styled-as-warn respectively;
    // pre-fix the single-arg path always called js_console_log_dynamic
    // and silently lost the stream distinction.
    module.declare_function("js_console_error_dynamic", VOID, &[DOUBLE]);
    module.declare_function("js_console_error_number", VOID, &[DOUBLE]);
    module.declare_function("js_console_warn_dynamic", VOID, &[DOUBLE]);
    module.declare_function("js_console_warn_number", VOID, &[DOUBLE]);
    // console.dir(value, options) — honors options.depth (#1199).
    module.declare_function("js_console_dir_with_options", VOID, &[DOUBLE, DOUBLE]);
    // console[dynamicKey] — resolve a console method by runtime key string to
    // the bound native closure (the `console[m](...)` computed-member form).
    module.declare_function("js_console_method_by_value", DOUBLE, &[DOUBLE]);

    // NaN-boxing wrappers (bridge between raw handles and NaN-boxed doubles).
    module.declare_function("js_nanbox_string", DOUBLE, &[I64]);
    module.declare_function("js_nanbox_pointer", DOUBLE, &[I64]);
    module.declare_function("js_nanbox_get_pointer", I64, &[DOUBLE]);
    module.declare_function(
        "js_native_handle_new_owned",
        DOUBLE,
        &[I64, I64, I32, I32, PTR, PTR, I64],
    );
    module.declare_function(
        "js_native_handle_new_borrowed",
        DOUBLE,
        &[I64, I64, I32, I32, PTR, I64],
    );
    module.declare_function(
        "js_native_handle_unwrap",
        I64,
        &[DOUBLE, I64, I32, I32, I32],
    );

    // Strings (enough to produce string literals for later phases).
    module.declare_function("js_string_from_bytes", I64, &[PTR, I32]);
    module.declare_function("js_string_from_wtf8_bytes", I64, &[PTR, I32]);

    // Type checks.
    module.declare_function("js_is_truthy", I32, &[DOUBLE]);
    module.declare_function("js_native_abi_check_f64", DOUBLE, &[DOUBLE]);
    module.declare_function("js_typed_f64_arg_guard", I32, &[DOUBLE]);
    module.declare_function("js_typed_f64_arg_to_raw", DOUBLE, &[DOUBLE]);
    module.declare_function("js_typed_i32_arg_guard", I32, &[DOUBLE]);
    module.declare_function("js_typed_i32_arg_to_raw", I32, &[DOUBLE]);
    module.declare_function("js_typed_i1_arg_guard", I32, &[DOUBLE]);
    module.declare_function("js_typed_i1_arg_to_raw", I32, &[DOUBLE]);
    module.declare_function("js_typed_string_arg_guard", I32, &[DOUBLE]);
    module.declare_function("js_typed_string_arg_to_raw", I64, &[DOUBLE]);
    module.declare_function("js_native_abi_check_f32", F32, &[DOUBLE]);
    module.declare_function("js_native_abi_check_i32", I32, &[DOUBLE]);
    module.declare_function("js_native_abi_check_i64", I64, &[DOUBLE]);
    module.declare_function("js_native_abi_check_u32", I32, &[DOUBLE]);
    module.declare_function("js_native_abi_check_u64", I64, &[DOUBLE]);
    module.declare_function("js_native_abi_check_usize", I64, &[DOUBLE]);
    module.declare_function("js_native_abi_check_string_ptr", I64, &[DOUBLE]);
    module.declare_function("js_native_abi_check_ptr", I64, &[DOUBLE]);
    module.declare_function("js_native_abi_check_buffer_data_ptr", PTR, &[DOUBLE]);
    module.declare_function("js_native_abi_check_buffer_byte_len", I64, &[DOUBLE]);
    module.declare_function("js_native_abi_check_promise", I64, &[DOUBLE]);
    module.declare_function("js_native_abi_check_pod_object", I64, &[DOUBLE]);

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
