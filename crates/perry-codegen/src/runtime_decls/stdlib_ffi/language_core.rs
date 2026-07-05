//! Core language / runtime FFI declarations (extracted from stdlib_ffi.rs):
//! Date, String, Object, Math, Atomics, Number, JSON, Map/Set, Error, Promise,
//! text encoding, closures, NaN-boxing, GC, console, fetch, net, performance,
//! async-step, slugify, class registration, runtime init/module-loader,
//! well-known Symbol hooks, Object.groupBy, JSX runtime adapter.

use super::*;
use crate::module::LlModule;
use crate::types::{DOUBLE, F32, I1, I16, I32, I64, I8, PTR, VOID};

pub(crate) fn declare_core(module: &mut LlModule) {
    // ========== Date ==========
    module.declare_function("js_date_to_locale_string", I64, &[DOUBLE]);
    // #600: number-form `(n).toLocaleString()` — formats with
    // thousands separators (en-US default). Routed by the
    // `Expr::DateToLocaleString` LLVM arm when the receiver's static
    // type narrows to `HirType::Number` / `HirType::Int32`.
    module.declare_function("js_number_to_locale_string", I64, &[DOUBLE]);
    // Runtime-dispatched `value.toLocaleString()` for receivers whose
    // static type is unknown at codegen time (plain objects, strings,
    // booleans). Returns an already-NaN-boxed value, so the LLVM arm
    // must NOT re-box it.
    module.declare_function("js_value_to_locale_string", DOUBLE, &[DOUBLE]);

    // ========== String ==========
    module.declare_function("js_string_split_regex", I64, &[I64, I64]);

    // ========== Object ==========
    module.declare_function("js_object_delete_dynamic", I32, &[I64, DOUBLE]);
    module.declare_function("js_object_get_prototype_of", DOUBLE, &[DOUBLE]);
    module.declare_function("js_object_set_prototype_of", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_object_define_properties", DOUBLE, &[DOUBLE, DOUBLE]);

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

    // ========== Atomics ==========
    module.declare_function("js_atomics_load", DOUBLE, &[PTR, DOUBLE, DOUBLE]);
    module.declare_function("js_atomics_is_lock_free", DOUBLE, &[PTR, DOUBLE]);
    module.declare_function("js_atomics_store", DOUBLE, &[PTR, DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_atomics_add", DOUBLE, &[PTR, DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_atomics_sub", DOUBLE, &[PTR, DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_atomics_and", DOUBLE, &[PTR, DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_atomics_or", DOUBLE, &[PTR, DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_atomics_xor", DOUBLE, &[PTR, DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function(
        "js_atomics_exchange",
        DOUBLE,
        &[PTR, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_atomics_compare_exchange",
        DOUBLE,
        &[PTR, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function("js_atomics_notify", DOUBLE, &[PTR, DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function(
        "js_atomics_wait",
        DOUBLE,
        &[PTR, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_atomics_wait_async",
        DOUBLE,
        &[PTR, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );

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
    module.declare_function("js_call_method", DOUBLE, &[DOUBLE, I64, I64, I64, I64]);
    module.declare_function("js_call_value", DOUBLE, &[DOUBLE, I64, I64]);
    // (closure_env i64, args_ptr, args_len i64). The args pointer is a real
    // pointer to a `[N x double]` stack buffer; declare it PTR (ABI-identical
    // to I64 in the integer register class) so call sites can pass an alloca
    // directly. See `try_lower_closure_call_fallthrough` (#3527).
    module.declare_function("js_closure_call_array", DOUBLE, &[I64, PTR, I64]);
    module.declare_function(
        "js_closure_call_apply_with_spread",
        DOUBLE,
        &[DOUBLE, PTR, I64, I64],
    );
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
    module.declare_function("js_console_context", DOUBLE, &[DOUBLE]);
    module.declare_function("js_console_create_task", DOUBLE, &[DOUBLE]);

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
    // Stashes the `fetch(url, { signal })` AbortSignal for the next
    // `js_fetch_with_options` so the request can be aborted.
    module.declare_function("js_fetch_set_pending_signal", VOID, &[DOUBLE]);
    // Headers-aware JSON stringify for the `fetch(url, { headers })` request
    // path: takes the headers value (f64) and returns a `*const StringHeader`
    // (i64) holding `{name:value}` JSON, treating a `Headers` handle safely.
    module.declare_function("js_fetch_headers_to_json", I64, &[DOUBLE]);

    // ========== Net ==========
    module.declare_function("js_net_create_connection", DOUBLE, &[I32, I64, I64]);
    // Issue #1123 followup — switched from `DOUBLE` to `I64` return.
    // Previous shape returned `id as f64` which arrived in user code
    // as a bare number; the receiver-unboxing path on `server.listen`
    // masked the lower 48 bits of `1.0` and got 0, so the listen FFI
    // ran with `handle=0` and silently bailed. Now we return the raw
    // handle as i64 and let codegen NaN-box with POINTER_TAG in
    // `expr.rs::Expr::NetCreateServer`, matching the
    // `js_node_http_create_server` (`I64, &[I64]`) convention.
    module.declare_function("js_net_create_server", I64, &[I64, I64]);
    module.declare_function("js_net_normalize_args", DOUBLE, &[DOUBLE]);
    module.declare_function(
        "js_net_create_server_handle_stub",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    // #2013: Node argument validation for the net surface. The createServer
    // options check takes the first positional arg as a NaN-boxed `DOUBLE`;
    // setTimeout takes (socket handle, msecs:DOUBLE, callback:I64).
    module.declare_function("js_net_validate_create_server_options", VOID, &[DOUBLE]);
    module.declare_function("js_net_socket_set_timeout", I64, &[I64, DOUBLE, I64]);
    // Issue #1123 followup — `net.Server` instance method FFIs. The
    // NA_PTR slot for callbacks is `I64` here (closures arrive as raw
    // pointer-bits after the codegen's `unbox_to_i64` lowering); ports
    // are `DOUBLE` because the codegen passes NA_F64 args as JS
    // numbers without unboxing. address() returns a `*mut StringHeader`
    // — `I64` at the FFI level.
    module.declare_function("js_net_server_listen", VOID, &[I64, DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_net_server_close", VOID, &[I64, I64]);
    module.declare_function("js_net_server_address", I64, &[I64]);
    module.declare_function("js_net_server_on", VOID, &[I64, I64, I64]);
    module.declare_function("js_net_server_get_listening", DOUBLE, &[I64]);
    module.declare_function("js_net_server_get_connections", DOUBLE, &[I64]);
    module.declare_function("js_net_server_get_max_connections", DOUBLE, &[I64]);
    module.declare_function("js_net_server_set_max_connections", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_net_server_get_drop_max_connection", DOUBLE, &[I64]);
    module.declare_function(
        "js_net_server_set_drop_max_connection",
        DOUBLE,
        &[I64, DOUBLE],
    );
    module.declare_function("js_net_block_list_new", I64, &[]);
    module.declare_function("js_net_block_list_is_block_list", DOUBLE, &[DOUBLE]);
    module.declare_function("js_net_block_list_add_address", DOUBLE, &[I64, I64, I64]);
    module.declare_function("js_net_block_list_add_range", DOUBLE, &[I64, I64, I64, I64]);
    module.declare_function(
        "js_net_block_list_add_subnet",
        DOUBLE,
        &[I64, I64, DOUBLE, I64],
    );
    module.declare_function("js_net_block_list_check", DOUBLE, &[I64, I64, I64]);
    module.declare_function("js_net_block_list_to_json", DOUBLE, &[I64]);
    module.declare_function("js_net_block_list_rules", I64, &[I64]);
    module.declare_function("js_net_block_list_from_json", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_net_socket_address_new", I64, &[DOUBLE]);
    module.declare_function("js_net_socket_address_parse", DOUBLE, &[I64]);
    module.declare_function("js_net_socket_address_get_address", I64, &[I64]);
    module.declare_function("js_net_socket_address_get_family", I64, &[I64]);
    module.declare_function("js_net_socket_address_get_port", DOUBLE, &[I64]);
    module.declare_function("js_net_socket_address_get_flowlabel", DOUBLE, &[I64]);
    module.declare_function("js_net_socket_get_type_of_service", DOUBLE, &[I64]);
    module.declare_function("js_net_socket_set_type_of_service", I64, &[I64, DOUBLE]);
    // Issue #2131 — net.Socket / net.Server lifecycle + EventEmitter
    // surface (lifecycle.rs in perry-ext-net). Listener-mutating
    // entry points all return the handle for chaining (Node's
    // semantics): the codegen NaN-boxes the I64 with POINTER_TAG via
    // NR_PTR. `address` / `eventNames` return raw StringHeader
    // pointers consumed by the NR_OBJ_FROM_JSON_STR pipeline.
    module.declare_function("js_net_socket_address", I64, &[I64]);
    module.declare_function("js_net_socket_once", I64, &[I64, I64, I64]);
    module.declare_function("js_net_socket_remove_listener", I64, &[I64, I64, I64]);
    module.declare_function("js_net_socket_remove_all_listeners", I64, &[I64, I64]);
    module.declare_function("js_net_socket_listener_count", DOUBLE, &[I64, I64]);
    module.declare_function("js_net_socket_event_names", I64, &[I64]);
    module.declare_function("js_net_socket_reset_and_destroy", I64, &[I64]);
    module.declare_function("js_net_server_once", I64, &[I64, I64, I64]);
    module.declare_function("js_net_server_remove_listener", I64, &[I64, I64, I64]);
    module.declare_function("js_net_server_remove_all_listeners", I64, &[I64, I64]);
    module.declare_function("js_net_server_listener_count", DOUBLE, &[I64, I64]);
    module.declare_function("js_net_server_event_names", I64, &[I64]);

    // ========== Performance ==========
    module.declare_function("js_performance_now", DOUBLE, &[]);
    // node:perf_hooks User Timing + ELU (perf_hooks.rs). All NaN-boxed f64.
    module.declare_function("js_perf_mark", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_perf_measure", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_perf_get_entries", DOUBLE, &[]);
    module.declare_function("js_perf_get_entries_by_type", DOUBLE, &[DOUBLE]);
    module.declare_function("js_perf_get_entries_by_name", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_perf_clear_marks", DOUBLE, &[DOUBLE]);
    module.declare_function("js_perf_clear_measures", DOUBLE, &[DOUBLE]);
    module.declare_function("js_perf_event_loop_utilization", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_perf_to_json", DOUBLE, &[]);
    module.declare_function("js_perf_clear_resource_timings", DOUBLE, &[]);
    module.declare_function("js_perf_set_resource_timing_buffer_size", DOUBLE, &[DOUBLE]);
    module.declare_function(
        "js_perf_mark_resource_timing",
        DOUBLE,
        &[
            DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE,
        ],
    );
    module.declare_function("js_perf_timerify", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_perf_observer_new", DOUBLE, &[DOUBLE]);
    module.declare_function("js_perf_observer_observe", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_perf_observer_disconnect", DOUBLE, &[DOUBLE]);
    module.declare_function("js_perf_observer_take_records", DOUBLE, &[DOUBLE]);
    // #1336: histogram stubs for perf_hooks.monitorEventLoopDelay() /
    // .createHistogram(). Histogram methods route via the perf_histogram
    // namespace through native_module_dispatch.
    module.declare_function("js_perf_monitor_event_loop_delay", DOUBLE, &[DOUBLE]);
    module.declare_function("js_perf_create_histogram", DOUBLE, &[DOUBLE]);
    module.declare_function("js_perf_histogram_noop", DOUBLE, &[]);
    module.declare_function("js_perf_histogram_percentile", DOUBLE, &[DOUBLE]);

    // ========== Async-step iter-result scratch (perf hot path) ==========
    // See promise.rs::ITER_RESULT_VALUE / ITER_RESULT_DONE — eliminates
    // the per-await {value, done} object alloc by stowing both fields
    // in a thread-local cell that the async-step driver consumes
    // immediately.
    module.declare_function("js_iter_result_set", DOUBLE, &[DOUBLE, I32]);
    module.declare_function("js_iter_result_set_f64", DOUBLE, &[DOUBLE, I32]);
    module.declare_function("js_iter_result_set_i32", DOUBLE, &[I32, I32]);
    module.declare_function("js_iter_result_set_i1", DOUBLE, &[I32, I32]);
    module.declare_function("js_iter_result_get_value", DOUBLE, &[]);
    module.declare_function("js_iter_result_get_value_f64", DOUBLE, &[]);
    module.declare_function("js_iter_result_get_value_i32", I32, &[]);
    module.declare_function("js_iter_result_get_value_i1", I32, &[]);
    module.declare_function("js_iter_result_get_done", DOUBLE, &[]);
    // Optimized async-step chain: replaces
    // `Promise.resolve(value).then(then_v_arrow, then_e_arrow)` in
    // the async-step driver by carrying `step_closure` directly
    // through the task queue.
    module.declare_function("js_async_step_chain", I64, &[DOUBLE, I64]);
    // Optimized async-step done: replaces `Promise.resolve(value)` in
    // the state-machine terminal branch by reusing the in-flight `next`
    // Promise (INLINE_TRAP_NEXT) when called from inside the microtask
    // runner dispatching this same step closure.
    module.declare_function("js_async_step_done", I64, &[DOUBLE, I64]);
    // #691 Phase 2: returns the live step closure pointer from
    // INLINE_TRAP.current_step TLS. Codegen NaN-boxes the result.
    module.declare_function("js_get_current_step_closure", I64, &[]);
    // #691 Phase 2: wrap the wrapper's initial step invocation with
    // TLS setup so `js_get_current_step_closure` inside the body sees
    // the right pointer on the very first state. Saves/restores
    // INLINE_TRAP across the call for nested-async composition.
    module.declare_function("js_async_first_call", DOUBLE, &[DOUBLE]);

    // ========== Slugify ==========
    module.declare_function("js_slugify", I64, &[I64]);
    module.declare_function("js_slugify_strict", I64, &[I64]);

    // ========== Class registration ==========
    module.declare_function("js_register_class_getter", VOID, &[I64, I64, I64, I64]);
    // Refs #486: per-class setter dispatch — see object.rs::js_register_class_setter.
    module.declare_function("js_register_class_setter", VOID, &[I64, I64, I64, I64]);
    // Default-aware spec `.length` per class method (CLASS_METHOD_BIND_LENGTHS).
    module.declare_function(
        "js_register_class_method_bind_length",
        VOID,
        &[I64, I64, I64, I64],
    );
    module.declare_function(
        "js_register_class_static_method_bind_length",
        VOID,
        &[I64, I64, I64, I64],
    );
    // Static accessors register on the class constructor (CLASS_STATIC_ACCESSORS).
    module.declare_function(
        "js_register_class_static_getter",
        VOID,
        &[I64, I64, I64, I64],
    );
    module.declare_function(
        "js_register_class_static_setter",
        VOID,
        &[I64, I64, I64, I64],
    );
    module.declare_function(
        "js_register_class_method",
        VOID,
        &[I64, I64, I64, I64, I64, I64, I64],
    );
    // #1787: register a class's standalone constructor so `new
    // <classObjectValue>()` can replay it on a dynamically-allocated instance.
    module.declare_function("js_register_class_constructor", VOID, &[I64, I64, I64]);
    // Constructor synth/rest flags: (class_id, has_synthetic_arguments,
    // has_rest) — consulted by the `super(...spread)` apply path so it packs a
    // pass-through parent ctor's `arguments` / rest slot correctly.
    module.declare_function(
        "js_register_class_constructor_flags",
        VOID,
        &[I64, I64, I64],
    );
    // #1788: register a class STATIC method + dispatch an inherited static
    // method on a class value (subclass extends a class-expression value).
    module.declare_function(
        "js_register_class_static_method",
        VOID,
        &[I64, I64, I64, I64, I64, I64],
    );
    module.declare_function(
        "js_class_static_method_call",
        DOUBLE,
        &[DOUBLE, I64, I64, PTR, I64],
    );
    // #446: bound-method closure for `obj.method` PropertyGet on a known class.
    // Lets `typeof obj.method === "function"` and `let f = obj.method; f(args)`
    // dispatch through CLASS_VTABLE_REGISTRY instead of returning undefined.
    module.declare_function("js_class_method_bind", DOUBLE, &[DOUBLE, I64, I64]);
    module.declare_function("js_class_method_bind_by_id", DOUBLE, &[DOUBLE, I64]);
    module.declare_function("js_class_prototype_method_value", DOUBLE, &[DOUBLE, DOUBLE]);
    // #519: read the implicit `this` thread-local set by
    // `js_native_call_method`'s field-scan dispatch when invoking a
    // closure-typed class field method-style. `Expr::This` codegen reads
    // this when the lexical this_stack is empty.
    module.declare_function("js_implicit_this_get", DOUBLE, &[]);
    module.declare_function("js_implicit_this_get_sloppy", DOUBLE, &[]);
    module.declare_function("js_implicit_this_set", DOUBLE, &[DOUBLE]);
    // Static-method prologue `this`: takes the one-shot receiver override
    // armed by dynamic static dispatch / call/apply, else returns the
    // lexical class-ref argument.
    module.declare_function("js_static_this_resolve", DOUBLE, &[DOUBLE]);
    module.declare_function("js_static_this_arm_classref", VOID, &[I32]);
    module.declare_function("js_static_this_arm_value", VOID, &[DOUBLE]);
    module.declare_function("js_ctor_return_override", DOUBLE, &[DOUBLE, DOUBLE, I32]);
    module.declare_function("js_new_target_get", DOUBLE, &[]);
    module.declare_function("js_new_target_set", DOUBLE, &[DOUBLE]);

    // ========== Runtime init / module loader ==========
    module.declare_function("js_get_export", DOUBLE, &[I64, I64, I64]);
    module.declare_function("js_get_property", DOUBLE, &[DOUBLE, I64, I64]);
    module.declare_function("js_load_module", I64, &[I64, I64]);
    module.declare_function("js_module_dynamic_import_apply_hooks", DOUBLE, &[DOUBLE]);
    module.declare_function(
        "js_native_call_method",
        DOUBLE,
        &[DOUBLE, I64, I64, I64, I64],
    );
    module.declare_function(
        "js_native_call_method_nullsafe",
        DOUBLE,
        &[DOUBLE, I64, I64, I64, I64],
    );
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
    // #809: string-key analog of `js_object_set_symbol_method`. Used by the
    // ordered-IIFE lowering of object literals that mix a spread with
    // `this`-binding methods (Effect `HashRing.ts` `Proto`). Sets the field
    // by name AND patches the closure's reserved (last) `this` capture slot
    // with the object, so a method written after a `...spread` still sees
    // the right receiver.
    module.declare_function(
        "js_object_set_method_by_name",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE],
    );
    // #2442: object-literal accessor installer for `{ get k(){}, set k(v){} }`.
    // Emitted by the IIFE lowering of object literals containing getters/setters.
    // Args: (obj, key, getter | undefined, setter | undefined). Merges a
    // separate get/set for the same key and rebinds `this` to obj.
    module.declare_function(
        "js_object_define_accessor",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_object_literal_set_computed",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function("js_object_literal_to_property_key", DOUBLE, &[DOUBLE]);
    module.declare_function("js_object_literal_set_prototype", DOUBLE, &[DOUBLE, DOUBLE]);
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
    // `Array.fromAsync(input, mapFn?, thisArg?)` — Node 22+. Dispatched at the LLVM
    // codegen level in `lower_call.rs` when the receiver is a global
    // and the property is `fromAsync`. The runtime function returns a
    // NaN-boxed Promise pointer; it awaits source values before optional
    // mapping and awaits mapped results before appending.
    // Arguments are NaN-boxed f64; runtime validates callback inputs and
    // rejects TypeError per Node. Object.groupBy → null-proto object (symbol
    // keys preserved); Map.groupBy → Map with un-coerced keys.
    module.declare_function("js_object_group_by", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_map_group_by", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_array_from_async", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);

    // ========== JSX runtime adapter (issue #277, #1653) ==========
    // `js_jsx(type, props)` and `js_jsxs(type, props)` are Perry's built-in
    // TSX/JSX runtime entry points. Codegen intercepts
    // ExternFuncRef { name: "jsx" } / "jsxs" in `lower_call.rs` and routes
    // them here with both args as DOUBLE (NaN-boxed), bypassing the string→PTR
    // conversion the generic path would apply to string literals. The runtime
    // handles HTML-style intrinsics, fragments, and function components.
    module.declare_function("js_jsx", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_jsxs", DOUBLE, &[DOUBLE, DOUBLE]);
}
