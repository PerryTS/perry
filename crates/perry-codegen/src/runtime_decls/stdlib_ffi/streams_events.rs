//! node:stream, EventEmitter, domain, StringDecoder, querystring, fastify,
//! nodemailer, rate-limit, validator stdlib FFI declarations
//! (extracted from stdlib_ffi.rs).

use crate::module::LlModule;
use crate::types::{DOUBLE, I32, I64, PTR};

pub(crate) fn declare_streams_events(module: &mut LlModule) {
    // ========== node:stream stubs (issue #631) ==========
    module.declare_function("js_event_emitter_subclass_init", DOUBLE, &[DOUBLE]); // #5137 EE subclass init
    module.declare_function(
        "js_event_emitter_async_resource_subclass_init",
        DOUBLE,
        &[DOUBLE, DOUBLE],
    );
    module.declare_function("js_array_subclass_init", DOUBLE, &[DOUBLE, DOUBLE]); // class extends Array
    module.declare_function("js_array_subclass_init_args", DOUBLE, &[DOUBLE, PTR, I64]);
    module.declare_function("js_map_set_subclass_init", DOUBLE, &[DOUBLE, I32, DOUBLE]); // class extends Map/Set
    module.declare_function(
        "js_weak_collection_subclass_init",
        DOUBLE,
        &[DOUBLE, I32, DOUBLE],
    );
    module.declare_function("js_promise_subclass_init", DOUBLE, &[DOUBLE, DOUBLE]); // class extends Promise
    module.declare_function("js_node_stream_readable_new", DOUBLE, &[DOUBLE]);
    module.declare_function(
        "js_node_stream_readable_subclass_init",
        DOUBLE,
        &[DOUBLE, DOUBLE],
    );
    module.declare_function("js_node_stream_writable_new", DOUBLE, &[DOUBLE]);
    module.declare_function(
        "js_node_stream_writable_subclass_init",
        DOUBLE,
        &[DOUBLE, DOUBLE],
    );
    module.declare_function("js_node_stream_duplex_new", DOUBLE, &[DOUBLE]);
    module.declare_function(
        "js_node_stream_duplex_subclass_init",
        DOUBLE,
        &[DOUBLE, DOUBLE],
    );
    module.declare_function("js_node_stream_transform_new", DOUBLE, &[DOUBLE]);
    module.declare_function(
        "js_node_stream_transform_subclass_init",
        DOUBLE,
        &[DOUBLE, DOUBLE],
    );
    module.declare_function("js_node_stream_passthrough_new", DOUBLE, &[DOUBLE]);
    module.declare_function(
        "js_node_stream_passthrough_subclass_init",
        DOUBLE,
        &[DOUBLE, DOUBLE],
    );
    module.declare_function("js_node_stream_readable_from", DOUBLE, &[DOUBLE]);
    module.declare_function(
        "js_node_stream_readable_from_options",
        DOUBLE,
        &[DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_node_stream_duplex_from_options",
        DOUBLE,
        &[DOUBLE, DOUBLE],
    );
    // #1534: static introspection helpers reflecting tracked stream state.
    module.declare_function("js_node_stream_is_disturbed", DOUBLE, &[DOUBLE]);
    module.declare_function("js_node_stream_is_errored", DOUBLE, &[DOUBLE]);
    module.declare_function("js_node_stream_is_readable", DOUBLE, &[DOUBLE]);
    module.declare_function("js_node_stream_is_writable", DOUBLE, &[DOUBLE]);
    // #2685: top-level stream helpers.
    module.declare_function("js_node_stream_is_array_buffer_view", DOUBLE, &[DOUBLE]);
    module.declare_function("js_node_stream_is_uint8_array", DOUBLE, &[DOUBLE]);
    module.declare_function("js_node_stream_is_destroyed", DOUBLE, &[DOUBLE]);
    module.declare_function("js_node_stream_uint8_array_to_buffer", DOUBLE, &[DOUBLE]);
    // #1537: getDefaultHighWaterMark(objectMode) / setDefaultHighWaterMark(objectMode, value).
    module.declare_function("js_node_stream_get_default_hwm", DOUBLE, &[DOUBLE]);
    module.declare_function("js_node_stream_set_default_hwm", DOUBLE, &[DOUBLE, DOUBLE]);
    // #1541: addAbortSignal(signal, stream) — identity-returns the stream.
    module.declare_function("js_node_stream_add_abort_signal", DOUBLE, &[DOUBLE, DOUBLE]);
    // #1539: compose(...streams) -> new Duplex; duplexPair(opts) -> [Duplex, Duplex].
    module.declare_function("js_node_stream_compose", DOUBLE, &[I64]);
    module.declare_function("js_node_stream_pipeline", DOUBLE, &[I64]);
    module.declare_function("js_node_stream_finished", DOUBLE, &[I64]);
    module.declare_function("js_node_stream_duplex_pair", DOUBLE, &[DOUBLE]);
    // #2521: Readable/Writable/Duplex .toWeb / .fromWeb adapters.
    module.declare_function("js_node_stream_readable_to_web", DOUBLE, &[DOUBLE]);
    module.declare_function("js_node_stream_writable_to_web", DOUBLE, &[DOUBLE]);
    module.declare_function("js_node_stream_duplex_to_web", DOUBLE, &[DOUBLE]);
    module.declare_function(
        "js_node_stream_readable_from_web",
        DOUBLE,
        &[DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_node_stream_writable_from_web",
        DOUBLE,
        &[DOUBLE, DOUBLE],
    );
    module.declare_function("js_node_stream_duplex_from_web", DOUBLE, &[DOUBLE, DOUBLE]);
    // Generic fallbacks for call sites without preserved stream class context.
    module.declare_function("js_node_stream_to_web", DOUBLE, &[DOUBLE]);
    module.declare_function("js_node_stream_from_web", DOUBLE, &[DOUBLE]);
    module.declare_function("js_node_stream_method_readable_aborted", DOUBLE, &[I64]);
    module.declare_function("js_node_stream_method_closed", DOUBLE, &[I64]);
    module.declare_function("js_node_stream_method_errored", DOUBLE, &[I64]);
    module.declare_function("js_node_stream_method_readable_did_read", DOUBLE, &[I64]);
    module.declare_function("js_node_stream_method_destroyed", DOUBLE, &[I64]);
    module.declare_function("js_node_stream_method_destroy", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_node_stream_method_pause", DOUBLE, &[I64]);
    module.declare_function("js_node_stream_method_readable", DOUBLE, &[I64]);
    module.declare_function("js_node_stream_method_readable_length", DOUBLE, &[I64]);
    module.declare_function("js_node_stream_method_readable_flowing", DOUBLE, &[I64]);
    module.declare_function("js_node_stream_method_readable_ended", DOUBLE, &[I64]);
    module.declare_function("js_node_stream_method_readable_object_mode", DOUBLE, &[I64]);
    module.declare_function("js_node_stream_method_pipe", DOUBLE, &[I64, DOUBLE, DOUBLE]);
    module.declare_function("js_node_stream_method_unpipe", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_node_stream_method_pause", DOUBLE, &[I64]);
    module.declare_function("js_node_stream_method_is_paused", DOUBLE, &[I64]);
    module.declare_function("js_node_stream_method_resume", DOUBLE, &[I64]);
    module.declare_function("js_node_stream_method_readable_encoding", DOUBLE, &[I64]);
    module.declare_function("js_node_stream_method_cork", DOUBLE, &[I64]);
    module.declare_function("js_node_stream_method_uncork", DOUBLE, &[I64]);
    module.declare_function("js_node_stream_method_writable_corked", DOUBLE, &[I64]);
    module.declare_function("js_node_stream_method_writable_length", DOUBLE, &[I64]);
    module.declare_function("js_node_stream_method_writable_need_drain", DOUBLE, &[I64]);
    module.declare_function("js_node_stream_method_writable", DOUBLE, &[I64]);
    module.declare_function("js_node_stream_method_writable_ended", DOUBLE, &[I64]);
    module.declare_function("js_node_stream_method_writable_finished", DOUBLE, &[I64]);
    module.declare_function("js_node_stream_method_allow_half_open", DOUBLE, &[I64]);
    module.declare_function("js_node_stream_method_set_encoding", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_node_stream_method_writable_object_mode", DOUBLE, &[I64]);

    // ========== Event emitter ==========
    module.declare_function("js_event_emitter_emit", DOUBLE, &[I64, I64, I64]);
    module.declare_function("js_event_emitter_emit0", DOUBLE, &[I64, I64]);
    module.declare_function("js_event_emitter_listener_count", DOUBLE, &[I64, I64, I64]);
    module.declare_function("js_event_emitter_new", I64, &[]);
    module.declare_function("js_event_emitter_new_with_options", I64, &[DOUBLE]);
    module.declare_function("js_event_emitter_on", I64, &[I64, I64, I64]);
    module.declare_function("js_event_emitter_once", I64, &[I64, I64, I64]);
    module.declare_function("js_event_emitter_prepend_listener", I64, &[I64, I64, I64]);
    module.declare_function(
        "js_event_emitter_prepend_once_listener",
        I64,
        &[I64, I64, I64],
    );
    module.declare_function("js_event_emitter_remove_all_listeners", I64, &[I64, I64]);
    module.declare_function("js_event_emitter_remove_listener", I64, &[I64, I64, I64]);
    module.declare_function("js_event_emitter_set_max_listeners", I64, &[I64, DOUBLE]);
    module.declare_function("js_event_emitter_get_max_listeners", DOUBLE, &[I64]);
    module.declare_function("js_event_emitter_event_names", I64, &[I64]);
    module.declare_function("js_event_emitter_listeners", I64, &[I64, I64]);
    module.declare_function("js_event_emitter_raw_listeners", I64, &[I64, I64]);
    module.declare_function("js_event_emitter_domain_value", DOUBLE, &[I64]);
    module.declare_function("js_event_emitter_async_resource_new", I64, &[DOUBLE]);
    module.declare_function("js_event_emitter_async_resource_call", DOUBLE, &[DOUBLE]);
    module.declare_function("js_event_emitter_async_resource_async_id", DOUBLE, &[I64]);
    module.declare_function(
        "js_event_emitter_async_resource_trigger_async_id",
        DOUBLE,
        &[I64],
    );
    module.declare_function(
        "js_event_emitter_async_resource_async_resource",
        DOUBLE,
        &[I64],
    );
    module.declare_function(
        "js_event_emitter_async_resource_emit_destroy",
        DOUBLE,
        &[I64],
    );
    // Module-level helpers
    module.declare_function("js_events_once", I64, &[DOUBLE, I64, DOUBLE]);
    module.declare_function("js_events_on", I64, &[DOUBLE, I64, DOUBLE]);
    module.declare_function("js_events_add_abort_listener", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_events_get_event_listeners", I64, &[DOUBLE, I64]);
    module.declare_function("js_events_listener_count", DOUBLE, &[DOUBLE, I64]);
    module.declare_function("js_events_get_max_listeners", DOUBLE, &[DOUBLE]);
    module.declare_function("js_events_set_max_listeners", DOUBLE, &[DOUBLE, I64]);
    module.declare_function("js_events_init", DOUBLE, &[]);

    // ========== Domain ==========
    module.declare_function("js_domain_create", I64, &[]);
    module.declare_function("js_domain_on", I64, &[I64, I64, I64]);
    module.declare_function("js_domain_emit", DOUBLE, &[I64, I64, I64]);
    module.declare_function("js_domain_run", DOUBLE, &[I64, DOUBLE, I64]);
    module.declare_function("js_domain_bind", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_domain_intercept", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_domain_add", I64, &[I64, DOUBLE]);
    module.declare_function("js_domain_remove", I64, &[I64, DOUBLE]);
    module.declare_function("js_domain_enter", DOUBLE, &[I64]);
    module.declare_function("js_domain_exit", DOUBLE, &[I64]);

    // ========== StringDecoder (issue #848) ==========
    // `js_string_decoder_new` allocates a real handle; `write` / `end`
    // are reachable both through the static NATIVE_MODULE_TABLE dispatch
    // (typed-receiver path: `const d = new StringDecoder("utf8");
    // d.write(buf)`) AND through HANDLE_METHOD_DISPATCH in
    // perry-stdlib's common/dispatch.rs (any-typed receiver fallback —
    // `(d as any).write(buf)`, `Map.get(...).write(...)`). Both routes
    // converge on `dispatch_string_decoder` in the stdlib. Property
    // getters `lastNeed` / `lastTotal` / `lastChar` only go through
    // HANDLE_PROPERTY_DISPATCH and need no static-call entry.
    module.declare_function("js_string_decoder_new", I64, &[I64]);
    module.declare_function("js_string_decoder_write", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_string_decoder_end", DOUBLE, &[I64, DOUBLE]);

    // ========== node:querystring ==========
    // Module-level functions (no receiver). `escape` / `unescape` take
    // a single NaN-boxed string and return one. `parse` returns a raw
    // ObjectHeader pointer (NaN-boxed at the call site via the
    // dispatcher's NR_PTR shape). `stringify` returns a NaN-boxed
    // STRING_TAG value directly.
    module.declare_function("js_querystring_escape", DOUBLE, &[DOUBLE]);
    module.declare_function("js_querystring_unescape", DOUBLE, &[DOUBLE]);
    module.declare_function("js_querystring_unescape_buffer", I64, &[DOUBLE, DOUBLE]);
    module.declare_function(
        "js_querystring_parse",
        I64,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_querystring_stringify",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );

    // ========== Nodemailer ==========
    module.declare_function("js_nodemailer_create_transport", DOUBLE, &[I64]);
    module.declare_function("js_nodemailer_send_mail", I64, &[I64, I64]);
    module.declare_function("js_nodemailer_verify", I64, &[I64]);
}
