//! Phase B string-operation declarations, part 2 — split out of strings.rs
//! to keep that file under the 2,000-line file-size gate (#1435). Pure
//! relocation of `module.declare_function(...)` calls; called at the tail of
//! `declare_phase_b_strings`.

use super::*;

/// Continuation of `declare_phase_b_strings` (see strings.rs).
pub(crate) fn declare_phase_b_strings_part2(module: &mut LlModule) {
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
    // Date.UTC(args_ptr, argc) — buffer of NaN-boxed args + count (#2826).
    module.declare_function("js_date_utc", DOUBLE, &[PTR, I32]);
    // `new Date(year, month, day?, hour?, min?, sec?, ms?)` (local time).
    module.declare_function(
        "js_date_new_local_components",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    // Unified Date setter entry point (#2851):
    // js_date_apply_setter(date, is_utc, field, args_ptr, argc). Replaces the
    // per-setter (date, value) helpers — `args` carries optional trailing
    // components.
    module.declare_function(
        "js_date_apply_setter",
        DOUBLE,
        &[DOUBLE, I32, I32, PTR, I32],
    );
    // Math extras (stubs in expr.rs had fallen through to no-op/passthrough).
    module.declare_function("js_math_clz32", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_cbrt", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_fround", DOUBLE, &[DOUBLE]);
    module.declare_function("js_math_f16round", DOUBLE, &[DOUBLE]);
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
    // new TextDecoder(label, fatal, ignoreBOM) -> handle
    module.declare_function("js_text_decoder_new", I64, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_text_encoder_encode_llvm", I64, &[DOUBLE]);
    // decode(handle, input) -> string ptr
    module.declare_function("js_text_decoder_decode_llvm", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_text_decoder_encoding", I64, &[DOUBLE]);
    module.declare_function("js_text_decoder_fatal", DOUBLE, &[DOUBLE]);
    module.declare_function("js_text_decoder_ignore_bom", DOUBLE, &[DOUBLE]);
    // Microtask queue (queueMicrotask / process.nextTick).
    module.declare_function("js_queue_microtask", VOID, &[I64]);
    module.declare_function("js_queue_next_tick", VOID, &[I64]);
    // #1351: process.nextTick(cb, ...args) — trailing args packed into a
    // stack buffer of doubles, forwarded when the tick fires.
    module.declare_function(
        "js_queue_next_tick_args",
        VOID,
        &[I64, crate::types::PTR, I32],
    );
    module.declare_function("js_drain_queued_microtasks", VOID, &[]);
    // Uint8Array constructor wrapper that flags the resulting buffer so the
    // formatter prints `Uint8Array(N) [ ... ]` instead of `<Buffer ...>`.
    module.declare_function("js_uint8array_from_array", I64, &[I64]);
    // `new Uint8Array(length)` — zero-filled BufferHeader marked as Uint8Array.
    module.declare_function("js_uint8array_alloc", I64, &[I32]);
    // `new Uint8Array(x)` runtime dispatch — handles the non-literal case
    // where `x` could be a number (length) or an array (source data).
    module.declare_function("js_uint8array_new", I64, &[DOUBLE]);
    module.declare_function("js_uint8array_view", I64, &[DOUBLE, I32, I32]);
    // Generic typed array runtime (Int8/16/32, Uint16/32, Float32/64, Uint8Clamped).
    // Uint8Array piggybacks on the BufferHeader path.
    module.declare_function("js_typed_array_new_empty", I64, &[I32, I32]);
    module.declare_function("js_typed_array_new_from_array", I64, &[I32, I64]);
    // Runtime-dispatched constructor: handles numeric length OR source-array arg.
    module.declare_function("js_typed_array_new", I64, &[I32, DOUBLE]);
    // #4103: `new TA(buffer, byteOffset, length?)` view constructor with spec
    // offset/length validation (kind, source, offset_value, length_value).
    module.declare_function("js_typed_array_view", I64, &[I32, DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_typed_array_length", I32, &[I64]);
    module.declare_function("js_typed_array_get", DOUBLE, &[I64, I32]);
    // #2063: string / dynamic-key `ta[key]` [[Get]] dispatcher (canonical
    // numeric index → element, else ordinary named-property [[Get]]).
    module.declare_function("js_typed_array_index_get_dynamic", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_typed_array_at", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_typed_array_set", VOID, &[I64, I32, DOUBLE]);
    module.declare_function(
        "js_typed_array_index_set_dynamic",
        DOUBLE,
        &[I64, DOUBLE, DOUBLE],
    );
    module.declare_function("js_uint8array_get", I32, &[I64, I32]);
    module.declare_function("js_uint8array_set", VOID, &[I64, I32, I32]);
    module.declare_function("js_native_arena_alloc", I64, &[I64]);
    module.declare_function("js_native_arena_view", I64, &[I64, I32, I64, I64]);
    module.declare_function("js_native_pod_view", I64, &[I64, I64, I64, I64, I64, I64]);
    module.declare_function("js_native_abi_check_pod_view_data_ptr", PTR, &[DOUBLE, I64]);
    module.declare_function(
        "js_native_abi_check_pod_view_record_count",
        I64,
        &[DOUBLE, I64],
    );
    module.declare_function("js_native_arena_dispose", VOID, &[I64]);
    module.declare_function("js_native_memory_fill_u32", VOID, &[I64, DOUBLE]);
    module.declare_function("js_native_memory_copy", VOID, &[I64, I64]);
    module.declare_function("js_typed_array_to_reversed", I64, &[I64]);
    module.declare_function("js_typed_array_to_sorted_default", I64, &[I64]);
    module.declare_function("js_typed_array_to_sorted_with_comparator", I64, &[I64, I64]);
    module.declare_function("js_typed_array_with", I64, &[I64, DOUBLE, DOUBLE]);
    module.declare_function("js_typed_array_find_last", DOUBLE, &[I64, I64]);
    module.declare_function("js_typed_array_find_last_index", DOUBLE, &[I64, I64]);
    // #3148: TypedArray.prototype.set(source, offset?) / subarray(begin?, end?).
    module.declare_function("js_typed_array_set_from", DOUBLE, &[I64, DOUBLE, DOUBLE]);
    module.declare_function(
        "js_typed_array_subarray",
        I64,
        &[I64, I32, DOUBLE, I32, DOUBLE],
    );
    // Object introspection / mutation (Agent A's accessor-descriptor work).
    module.declare_function("js_object_has_own", DOUBLE, &[DOUBLE, DOUBLE]);
    // #2891: Object.prototype.propertyIsEnumerable.call(obj, key).
    module.declare_function(
        "js_object_property_is_enumerable",
        DOUBLE,
        &[DOUBLE, DOUBLE],
    );
    // Annex B §B.2.2 accessor helpers — `Object.prototype.__defineGetter__`
    // etc., reached via the `.call(obj, key[, fn])` rewrite.
    module.declare_function("js_object_define_getter", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_object_define_setter", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_object_lookup_getter", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_object_lookup_setter", DOUBLE, &[DOUBLE, DOUBLE]);
    // Issue #620: own-property override probe used by class-method dispatch.
    // Returns the stored value if `name` is in obj's own keys_array (data
    // property only — no vtable getter walk), else TAG_UNDEFINED. Lets
    // dispatch detect `this.method = X` overrides at the call site.
    module.declare_function(
        "js_object_get_own_field_or_undef",
        DOUBLE,
        &[DOUBLE, PTR, I64],
    );
    // Issue #629: stub for unresolved namespace imports — returns a stable
    // empty-object pointer so `typeof ns === "object"` and `ns.method`
    // cleanly resolves to undefined (instead of TAG_TRUE → "boolean" /
    // "(boolean).method is not a function").
    module.declare_function("js_unresolved_namespace_stub", DOUBLE, &[]);
    // Issue #841: per-(submodule, export) function-singleton getter for
    // the five Node submodules without perry-stdlib backing. Returns a
    // NaN-boxed ClosureHeader pointer (typeof "function") or TAG_TRUE
    // as a fallback if the (submod_key, name) pair isn't registered.
    module.declare_function(
        "js_node_submodule_export_as_function",
        DOUBLE,
        &[PTR, I32, PTR, I32],
    );
    // Namespace member reads for known Node submodules. Unlike direct
    // named-import fallback, missing properties return undefined.
    module.declare_function(
        "js_node_submodule_namespace_member",
        DOUBLE,
        &[PTR, I32, PTR, I32],
    );
    // Issue #841 companion: per-submodule namespace stub object. Returns
    // a NaN-boxed ObjectHeader pointer whose fields are the function
    // singletons emitted by `js_node_submodule_export_as_function`.
    module.declare_function("js_node_submodule_namespace", DOUBLE, &[PTR, I32]);
    // Submodule devirt installs (devirt phase 3)
    module.declare_function("js_node_submod_install_vm", VOID, &[]);
    module.declare_function("js_node_submod_install_timers", VOID, &[]);
    module.declare_function("js_node_submod_install_timers_promises", VOID, &[]);
    module.declare_function("js_node_submod_install_fs_promises", VOID, &[]);
    module.declare_function("js_node_submod_install_readline_promises", VOID, &[]);
    module.declare_function("js_node_submod_install_stream_promises", VOID, &[]);
    module.declare_function("js_node_submod_install_stream_consumers", VOID, &[]);
    module.declare_function("js_node_submod_install_stream_web", VOID, &[]);
    module.declare_function("js_node_submod_install_hono_jsx_server", VOID, &[]);
    module.declare_function("js_node_submod_install_hono_jsx_streaming", VOID, &[]);
    module.declare_function("js_node_submod_install_sys", VOID, &[]);
    module.declare_function("js_node_submod_install_diagnostics_channel", VOID, &[]);
    module.declare_function("js_node_submod_install_trace_events", VOID, &[]);
    module.declare_function("js_node_submod_install_test", VOID, &[]);
    module.declare_function("js_node_submod_install_test_reporters", VOID, &[]);
    module.declare_function("js_node_submod_install_all", VOID, &[]);
    module.declare_function("js_node_submod_enable_install_all", VOID, &[]);
    // Issue #692: stub for default-imported callables from unresolved modules —
    // returns NaN-boxed undefined and prints a one-shot diagnostic, so the
    // program links instead of failing with `undefined reference to 'default'`.
    module.declare_function("js_unresolved_default_call", DOUBLE, &[]);
    // Issue #611: real persistent globalThis singleton. Returns a
    // NaN-boxed POINTER to a per-process ObjectHeader so
    // `globalThis[k] = v` then `globalThis[k]` round-trips correctly.
    // The codegen IndexGet/IndexSet paths on `Expr::GlobalGet` route
    // through this helper.
    module.declare_function("js_get_global_this", DOUBLE, &[]);
    module.declare_function("js_module_top_this", DOUBLE, &[]);
    module.declare_function("js_global_or_console_property_by_name", DOUBLE, &[I64]);
    // Refs #420: register a static computed-key Symbol field on a class.
    // Called from `init_static_fields` for each `static [Symbol.X] = init`.
    module.declare_function(
        "js_class_register_static_symbol",
        VOID,
        &[I32, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_register_class_computed_method",
        VOID,
        &[I64, DOUBLE, I64, I64, I64, I64],
    );
    module.declare_function(
        "js_register_class_computed_accessor",
        VOID,
        &[I64, DOUBLE, I64, I64, I64],
    );
    // v0.5.747: register a string-named static field on a class so reads
    // via the runtime dynamic-dispatch path (when the class ref is in an
    // Any-typed local) find the value. Refs #420 / #618 followup.
    module.declare_function(
        "js_class_register_static_field",
        VOID,
        &[I32, PTR, I64, DOUBLE],
    );
    module.declare_function(
        "js_object_define_property",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_object_get_own_property_descriptor",
        DOUBLE,
        &[DOUBLE, DOUBLE],
    );
    module.declare_function("js_object_get_own_property_descriptors", DOUBLE, &[DOUBLE]);
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
    module.declare_function(
        "js_object_set_symbol_property",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function("js_to_property_key", DOUBLE, &[DOUBLE]);
    module.declare_function(
        "js_object_set_property_key",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function("js_object_get_property_key", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function(
        "js_object_set_property_key_method",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function("js_object_super_get", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function(
        "js_object_super_put_value_set",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE, I32],
    );
    module.declare_function(
        "js_object_super_call",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, PTR, I64],
    );
    module.declare_function(
        "js_object_literal_infer_computed_function_name",
        DOUBLE,
        &[DOUBLE, DOUBLE],
    );
    module.declare_function("js_object_get_symbol_property", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_object_create", DOUBLE, &[DOUBLE]);
    // #2816: Object.create(proto[, propertiesObject]) — validates the
    // prototype and applies the optional descriptor bag.
    module.declare_function("js_object_create_with_props", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_object_freeze", DOUBLE, &[DOUBLE]);
    module.declare_function("js_object_seal", DOUBLE, &[DOUBLE]);
    module.declare_function("js_object_prevent_extensions", DOUBLE, &[DOUBLE]);
    // Object spread: copy all own fields from src into dst.
    module.declare_function("js_object_copy_own_fields", VOID, &[I64, DOUBLE]);
    // Object.assign(target, source): mutate target with source's own
    // string-keyed AND symbol-keyed enumerable properties; returns target.
    // Refs #590.
    module.declare_function("js_object_assign_validate_target", DOUBLE, &[DOUBLE]);
    module.declare_function("js_object_assign_one", DOUBLE, &[DOUBLE, DOUBLE]);
    // String extras (already in string.rs; expr.rs was stubbing or missing dispatch).
    module.declare_function("js_string_at", DOUBLE, &[I64, I32]);
    module.declare_function("js_string_code_point_at", DOUBLE, &[I64, I32]);
    // #2788: take the raw NaN-boxed f64 so the runtime can apply ToUint16
    // (fromCharCode) / RangeError validation (fromCodePoint) — a prior fptosi
    // truncated fractional/non-finite inputs before they could be observed.
    module.declare_function("js_string_from_code_point", I64, &[DOUBLE]);
    // Callable String.raw(callSite, substitutionsArray) -> string (#2789)
    module.declare_function("js_string_raw", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_string_from_char_code", I64, &[DOUBLE]);
    module.declare_function("js_string_from_char_code_array", I64, &[DOUBLE]);
    module.declare_function("js_string_char_code_at", DOUBLE, &[I64, I32]);
    module.declare_function("js_string_last_index_of", I32, &[I64, I64]);
    module.declare_function(
        "js_string_last_index_of_from",
        I32,
        &[I64, I64, DOUBLE, I32],
    );
    module.declare_function("js_string_locale_compare", DOUBLE, &[I64, I64]);
    module.declare_function("js_string_locale_compare_opts", DOUBLE, &[I64, I64, DOUBLE]);
    module.declare_function("js_string_normalize", I64, &[I64, DOUBLE]);
    module.declare_function("js_string_pad_start", I64, &[I64, DOUBLE, I64]);
    module.declare_function("js_string_pad_end", I64, &[I64, DOUBLE, I64]);
    // ToString-coerce a padStart/padEnd fillString (undefined → null → " ").
    module.declare_function("js_string_pad_fill", I64, &[DOUBLE]);
    module.declare_function("js_string_is_well_formed", DOUBLE, &[I64]);
    module.declare_function("js_string_to_well_formed", I64, &[I64]);
    module.declare_function("js_string_match_all", I64, &[I64, I64]);
    module.declare_function("js_string_match_all_value", I64, &[I64, DOUBLE]);
    module.declare_function("js_string_search_regex", I32, &[I64, I64]);
    // Boxed-arg search/match: coerce a non-RegExp arg via RegExpCreate(ToString)
    // (ECMA-262 §22.1.3.12 / §22.1.3.11).
    module.declare_function("js_string_search_value", I32, &[I64, DOUBLE]);
    module.declare_function("js_string_match_value", I64, &[I64, DOUBLE]);
    // Regex extras (runtime has them; codegen was stubbing).
    module.declare_function("js_regexp_exec_get_index", DOUBLE, &[]);
    module.declare_function("js_regexp_exec_get_groups", I64, &[]);
    module.declare_function("js_regexp_get_last_index", DOUBLE, &[I64]);
    module.declare_function("js_regexp_set_last_index", VOID, &[I64, DOUBLE]);
    module.declare_function("js_regexp_get_source", I64, &[I64]);
    module.declare_function("js_regexp_get_flags", I64, &[I64]);
    module.declare_function("js_string_replace_regex_named", I64, &[I64, I64, I64]);
    module.declare_function("js_string_replace_all_regex_named", I64, &[I64, I64, I64]);
    module.declare_function("js_string_replace_string_fn", I64, &[I64, I64, DOUBLE]);
    module.declare_function("js_string_replace_string_dyn", I64, &[I64, I64, DOUBLE]);
    module.declare_function("js_string_replace_all_string_dyn", I64, &[I64, I64, DOUBLE]);
    module.declare_function("js_string_replace_regex_dyn", I64, &[I64, I64, DOUBLE]);
    module.declare_function("js_string_replace_all_regex_dyn", I64, &[I64, I64, DOUBLE]);
    module.declare_function("js_string_replace_search_dyn", I64, &[I64, DOUBLE, DOUBLE]);
    module.declare_function(
        "js_string_replace_all_search_dyn",
        I64,
        &[I64, DOUBLE, DOUBLE],
    );
    module.declare_function("js_string_replace_all_string_fn", I64, &[I64, I64, DOUBLE]);
    module.declare_function("js_string_replace_regex_fn", I64, &[I64, I64, DOUBLE]);
    module.declare_function("js_string_replace_all_regex_fn", I64, &[I64, I64, DOUBLE]);
    // structuredClone(v[, options]) — real deep copy, with ArrayBuffer transfer.
    module.declare_function("js_structured_clone", DOUBLE, &[DOUBLE]);
    module.declare_function(
        "js_structured_clone_with_options",
        DOUBLE,
        &[DOUBLE, DOUBLE],
    );
    // #4141: generator/async-generator instance `[[Prototype]]` linker.
    module.declare_function("js_generator_attach_prototype", DOUBLE, &[DOUBLE, I32]);
    module.declare_function(
        "js_generator_attach_closure_prototype",
        DOUBLE,
        &[DOUBLE, I64],
    );
    // WeakRef / FinalizationRegistry (weakref.rs). `js_weakref_new` /
    // `js_finreg_new` return raw `*mut ObjectHeader` (i64 pointer, must be
    // POINTER_TAG-boxed at the call site). The deref/register/unregister
    // helpers already return NaN-tagged f64 values.
    module.declare_function("js_weakref_new", I64, &[DOUBLE]);
    module.declare_function("js_weakref_deref", DOUBLE, &[DOUBLE]);
    module.declare_function("js_finreg_new", I64, &[DOUBLE]);
    module.declare_function(
        "js_finreg_register",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
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
    // #2838/#2836: full AggregateError ctor — errors as raw value, options.
    module.declare_function("js_aggregateerror_new_full", I64, &[DOUBLE, I64, DOUBLE]);
    // #2836: Error/subclass ctor honoring a runtime `{ cause }` options value.
    module.declare_function("js_error_new_kind_with_options", I64, &[I32, I64, DOUBLE]);
    // #2904: Error.isError(value) duck-check.
    module.declare_function("js_error_is_error", DOUBLE, &[DOUBLE]);
    // AggregateError.errors field access — returns raw *ArrayHeader.
    module.declare_function("js_error_get_errors", I64, &[I64]);
    // Crypto stdlib — sha256/md5/hmac/randomBytes/randomUUID used by
    // the expr.rs chain collapse for createHash().update().digest().
    module.declare_function("js_crypto_sha256", I64, &[I64]);
    module.declare_function("js_crypto_sha256_bytes", I64, &[I64]);
    module.declare_function("js_crypto_md5", I64, &[I64]);
    module.declare_function("js_crypto_hmac_sha256", I64, &[I64, I64]);
    module.declare_function("js_crypto_hmac_sha256_bytes", I64, &[I64, I64]);
    // #2013/#3146: shared codegen-callable argument validators. Emitted before
    // a NaN-boxed value is unboxed to a raw pointer so a bad type throws node's
    // `TypeError [ERR_INVALID_ARG_TYPE]` instead of dereferencing a bogus
    // pointer (segfault). Used by `crypto.createHash`/`createHmac`/`pbkdf2*`.
    module.declare_function("js_runtime_validate_string_arg", VOID, &[DOUBLE, PTR, I32]);
    // #5247: source-location tracking for the dynamic call-dispatch throw
    // path. Emitted before a call's dispatch only under `--debug-symbols`.
    module.declare_function("js_set_call_location", VOID, &[PTR, I64, I32]);
    module.declare_function(
        "js_runtime_validate_crypto_key_arg",
        VOID,
        &[DOUBLE, PTR, I32],
    );
    module.declare_function(
        "js_runtime_validate_integer_arg",
        VOID,
        &[DOUBLE, PTR, I32, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_crypto_pbkdf2_bytes",
        I64,
        &[I64, I64, DOUBLE, DOUBLE, I64],
    );
    module.declare_function(
        "js_crypto_pbkdf2_async_alg",
        DOUBLE,
        &[I64, I64, DOUBLE, DOUBLE, I64, DOUBLE],
    );
    module.declare_function("js_crypto_argon2_sync", I64, &[I64, DOUBLE]);
    module.declare_function("js_crypto_argon2_async", DOUBLE, &[I64, DOUBLE, DOUBLE]);
    module.declare_function("js_crypto_encapsulate", I64, &[DOUBLE]);
    module.declare_function("js_crypto_encapsulate_async", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_crypto_decapsulate", I64, &[DOUBLE, DOUBLE]);
    module.declare_function(
        "js_crypto_decapsulate_async",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_crypto_hkdf_bytes_alg",
        I64,
        &[I64, I64, I64, I64, DOUBLE],
    );
    module.declare_function(
        "js_crypto_hkdf_async_alg",
        DOUBLE,
        &[I64, I64, I64, I64, DOUBLE, DOUBLE],
    );
    module.declare_function("js_crypto_scrypt_bytes", I64, &[I64, I64, DOUBLE, I64]);
    module.declare_function(
        "js_crypto_scrypt_async",
        DOUBLE,
        &[I64, I64, DOUBLE, DOUBLE],
    );
    module.declare_function("js_crypto_sign_rsa_sha256", I64, &[I64, I64, DOUBLE]);
    module.declare_function("js_crypto_sign_async", DOUBLE, &[I64, I64, DOUBLE, DOUBLE]);
    module.declare_function(
        "js_crypto_verify_rsa_sha256",
        DOUBLE,
        &[I64, I64, DOUBLE, I64],
    );
    module.declare_function(
        "js_crypto_verify_async",
        DOUBLE,
        &[I64, I64, DOUBLE, I64, DOUBLE],
    );
    module.declare_function("js_crypto_public_encrypt", I64, &[I64, I64]);
    module.declare_function("js_crypto_private_decrypt", I64, &[I64, I64]);
    module.declare_function("js_crypto_private_encrypt", I64, &[I64, I64]);
    module.declare_function("js_crypto_public_decrypt", I64, &[I64, I64]);
    module.declare_function("js_crypto_create_public_key", I64, &[I64]);
    module.declare_function("js_crypto_create_private_key_value", I64, &[DOUBLE]);
    module.declare_function("js_crypto_create_public_key_value", I64, &[DOUBLE]);
    module.declare_function("js_crypto_generate_key_pair_sync_rsa", I64, &[DOUBLE]);
    module.declare_function("js_crypto_generate_key_pair_sync_ec_p256", I64, &[DOUBLE]);
    module.declare_function("js_crypto_generate_key_pair_sync_ed25519", I64, &[DOUBLE]);
    module.declare_function("js_crypto_generate_key_pair_sync_x25519", I64, &[DOUBLE]);
    module.declare_function(
        "js_crypto_generate_key_pair_async",
        DOUBLE,
        &[I64, DOUBLE, DOUBLE],
    );
    module.declare_function("js_crypto_diffie_hellman", I64, &[DOUBLE]);
    module.declare_function("js_crypto_get_cipher_info", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_crypto_get_curves", I64, &[]);
    module.declare_function("js_crypto_secure_heap_used", I64, &[]);
    module.declare_function("js_crypto_random_bytes_buffer", I64, &[DOUBLE]);
    module.declare_function("js_crypto_random_bytes_async", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_crypto_random_uuid", I64, &[DOUBLE]);
    module.declare_function("js_crypto_random_uuidv7", I64, &[]);
    // crypto.randomInt([min,] max[, cb]) -> number; codegen passes min=0 for the
    // single-arg form. Returns the integer as a plain double.
    module.declare_function("js_crypto_random_int", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function(
        "js_crypto_random_int_async",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE],
    );
    // crypto.timingSafeEqual(a, b) -> boolean (NaN-boxed). Args stay boxed so
    // stdlib can validate BufferSource types before reading bytes.
    module.declare_function("js_crypto_timing_safe_equal", DOUBLE, &[DOUBLE, DOUBLE]);
    // crypto.getHashes() / getCiphers() -> string[]; returns *mut ArrayHeader.
    module.declare_function("js_crypto_get_hashes", I64, &[]);
    module.declare_function("js_crypto_get_ciphers", I64, &[]);
    module.declare_function("js_crypto_generate_prime_sync", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function(
        "js_crypto_generate_prime_async",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function("js_crypto_check_prime_sync", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function(
        "js_crypto_check_prime_async",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE],
    );
    // `crypto.createSecretKey(key, encoding?)` — returns Uint8Array-marked
    // BufferHeader of the key bytes (jose accepts Uint8Array for HS*).
    module.declare_function("js_crypto_create_secret_key", I64, &[I64, I64]);
    module.declare_function("js_crypto_generate_key_sync", I64, &[I64, DOUBLE]);
    module.declare_function(
        "js_crypto_generate_key_async",
        DOUBLE,
        &[I64, DOUBLE, DOUBLE],
    );
    // Web Crypto (issue #561): crypto.subtle.{digest,importKey,sign,verify}.
    // Each takes NaN-boxed JS values as f64 and returns a *mut Promise.
    module.declare_function("js_webcrypto_digest", I64, &[DOUBLE, DOUBLE]);
    module.declare_function(
        "js_webcrypto_import_key",
        I64,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function("js_webcrypto_export_key", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_webcrypto_sign", I64, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function(
        "js_webcrypto_verify",
        I64,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function("js_webcrypto_derive_bits", I64, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function(
        "js_webcrypto_derive_key",
        I64,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    // AES-GCM encrypt / decrypt (issue #561 follow-up). Same Promise
    // shape as sign/verify; runtime resolves synchronously.
    module.declare_function("js_webcrypto_encrypt", I64, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_webcrypto_decrypt", I64, &[DOUBLE, DOUBLE, DOUBLE]);
    // subtle.generateKey(algorithm, extractable, usages) → Promise<CryptoKey>.
    // Initial implementation covers AES-GCM (128/256-bit) — the shape
    // jose's `generateSecret('A256GCM')` reaches for.
    module.declare_function("js_webcrypto_generate_key", I64, &[DOUBLE, DOUBLE, DOUBLE]);
    // subtle.wrapKey(format, key, wrappingKey, wrapAlgorithm) →
    // Promise<Uint8Array>. Initial implementation covers AES-KW
    // (`{ name: 'AES-KW' }`) plus AES-GCM (`{ name: 'AES-GCM', iv }`).
    // Required by jose's `wrapKey`.
    module.declare_function(
        "js_webcrypto_wrap_key",
        I64,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    // subtle.unwrapKey(format, wrappedKey, unwrappingKey, unwrapAlgorithm,
    //   unwrappedKeyAlgorithm, extractable, usages) → Promise<CryptoKey>.
    module.declare_function(
        "js_webcrypto_unwrap_key",
        I64,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    // `zlib.createBrotliDecompress(options?)` — axios feature-check
    // shim. Returns a registered Buffer-shaped handle (NaN-boxed at
    // the call site).
    module.declare_function("js_zlib_create_brotli_decompress", I64, &[DOUBLE]);
    // crypto.randomFillSync(buf, offset?, size?) → returns the same
    // NaN-boxed buffer with random bytes written in-place.
    module.declare_function(
        "js_crypto_random_fill_sync",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_crypto_random_fill_async",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    // Hash-handle form (issue #86): `const h = crypto.createHash(alg);
    // h.update(x); h.digest()`. Returns a NaN-boxed POINTER_TAG handle id;
    // subsequent method dispatch flows through HANDLE_METHOD_DISPATCH.
    module.declare_function("js_crypto_create_hash", DOUBLE, &[I64]);
    module.declare_function("js_crypto_create_hash_options", DOUBLE, &[I64, DOUBLE]);
    // #1367: `new X509Certificate(pem|der)` — parses to a NaN-boxed handle.
    module.declare_function("js_crypto_x509_new", DOUBLE, &[I64]);
    module.declare_function("js_crypto_certificate_verify_spkac", DOUBLE, &[DOUBLE]);
    module.declare_function("js_crypto_certificate_export_public_key", DOUBLE, &[DOUBLE]);
    module.declare_function("js_crypto_certificate_export_challenge", DOUBLE, &[DOUBLE]);
    module.declare_function("js_crypto_create_sign", DOUBLE, &[I64]);
    module.declare_function("js_crypto_create_verify", DOUBLE, &[I64]);
    module.declare_function("js_crypto_create_ecdh", DOUBLE, &[I64]);
    module.declare_function(
        "js_crypto_create_diffie_hellman",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function("js_crypto_get_diffie_hellman", DOUBLE, &[DOUBLE]);
    module.declare_function(
        "js_crypto_ecdh_convert_key",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    // Hmac-handle form (issue #1076): `crypto.createHmac(alg, key).update(d).
    // digest(enc)` when `alg` isn't a literal string the chain-collapse
    // recognizes (`const alg = "sha256"`, for-of bindings, ternaries, etc.).
    // Same handle protocol as `js_crypto_create_hash` — POINTER_TAG box, then
    // HANDLE_METHOD_DISPATCH routes `.update` / `.digest` to `dispatch_hmac`.
    module.declare_function("js_crypto_create_hmac", DOUBLE, &[I64, I64]);
    module.declare_function("js_string_from_bytes", I64, &[I64, I32]);
    module.declare_function("js_string_from_wtf8_bytes", I64, &[I64, I32]);
    // Buffer.alloc(size, fill) — returns raw *mut BufferHeader.
    module.declare_function("js_buffer_alloc", I64, &[I32, I32]);
    module.declare_function("js_buffer_alloc_fill_value", I64, &[I32, DOUBLE, I32]);
    // Issue #579: `new ArrayBuffer(size)` — zero-filled BufferHeader of `size`
    // bytes that subsequent `new Uint8Array(ab)` views ALIAS via shared pointer.
    module.declare_function("js_array_buffer_new", I64, &[I32]);
    module.declare_function("js_shared_array_buffer_new", I64, &[I32]);
    module.declare_function("js_array_buffer_new_value", I64, &[DOUBLE]);
    module.declare_function("js_shared_array_buffer_new_value", I64, &[DOUBLE]);
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
    // Issue #586: ECMAScript thenable assimilation for `await`. Takes a
    // NaN-boxed f64; returns either the same value (real Promise / non-
    // thenable) or a fresh wrapper Promise that the await loop polls
    // (when the operand is an object whose class chain has a `.then`
    // method). Caller must call this before the `js_value_is_promise`
    // branch so `await thenable` enters the polling path.
    module.declare_function("js_assimilate_thenable", DOUBLE, &[DOUBLE]);
    module.declare_function("js_promise_run_microtasks", I32, &[]);
    // ESM entry marker: first microtask drain finishes promise jobs before
    // the nextTick queue (Node module-evaluation checkpoint ordering, #788).
    module.declare_function("js_mark_entry_module_esm", VOID, &[]);
    // Promise/queueMicrotask jobs only — no nextTick drain, no timers.
    // Used by the await lowering's drain_once block (#788).
    module.declare_function("js_promise_run_promise_jobs", I32, &[]);
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
    module.declare_function("js_unsettled_top_level_await_exit", VOID, &[]);
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
    // setjmp variant selection:
    //   - Windows MSVC requires _setjmp(buf, frame_ptr)
    //   - Apple targets: the default C `setjmp(3)` saves the signal mask
    //     via a `sigprocmask` syscall (and the alt-signal-stack via
    //     `__sigaltstack`) — together those syscalls dominate CPU for
    //     async-heavy workloads (~43% of CPU on promise_all_chains.ts
    //     before the swap). Perry never longjmps out of a signal
    //     handler, so the fast `_setjmp(3)` (no sigprocmask) is
    //     functionally equivalent for our exception path. The LLVM-IR
    //     name `_setjmp` maps to the Mach-O linker symbol `__setjmp`
    //     (the C ABI prepends an underscore), which is the fast
    //     variant in libsystem_platform.dylib.
    //   - Linux glibc: the C `setjmp(3)` already does NOT save the
    //     signal mask (POSIX leaves it implementation-defined;
    //     `sigsetjmp(env, 1)` is the signal-saving variant on Linux).
    //     So `setjmp` on Linux is already the fast path; no swap
    //     needed.
    if cfg!(target_os = "windows") {
        module.declare_function("_setjmp", I32, &[PTR, PTR]);
    } else if cfg!(target_vendor = "apple") {
        module.declare_function("_setjmp", I32, &[PTR]);
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
    module.declare_function("js_timer_tick_if_refed", I32, &[]);
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
    // #591: returns 1 iff perry-runtime's per-thread microtask
    // TASK_QUEUE has a pending entry. The codegen-emitted event-loop
    // header check ORs this in so the loop doesn't exit between the
    // body iteration that queues a chained `.then` callback and the
    // next body iteration's microtask drain.
    module.declare_function("js_microtasks_pending", I32, &[]);
    // #2013 — validate setTimeout/setInterval/setImmediate's first arg
    // (callback), returning the unboxed pointer for the valid case and
    // throwing TypeError ERR_INVALID_ARG_TYPE for everything else.
    module.declare_function("js_timer_validate_callback", I64, &[DOUBLE, I32]);
    module.declare_function("js_set_timeout_callback", I64, &[I64, DOUBLE]);
    // Refs #665: `setTimeout(fn, delay, ...args)` with trailing args. The
    // args are packed into a stack buffer of doubles at the call site and
    // forwarded by index when the timer fires. Used by Promise-executor
    // patterns like `setTimeout(resolve, delay, res)`.
    module.declare_function(
        "js_set_timeout_callback_args",
        I64,
        &[I64, DOUBLE, crate::types::PTR, I32],
    );
    module.declare_function("js_set_immediate_callback", I64, &[I64]);
    module.declare_function(
        "js_set_immediate_callback_args",
        I64,
        &[I64, crate::types::PTR, I32],
    );
    module.declare_function("setInterval", I64, &[I64, DOUBLE]);
    module.declare_function(
        "js_set_interval_callback_args",
        I64,
        &[I64, DOUBLE, crate::types::PTR, I32],
    );
    module.declare_function("clearTimeout", VOID, &[I64]);
    module.declare_function("clearInterval", VOID, &[I64]);
    module.declare_function("clearImmediate", VOID, &[I64]);
    module.declare_function("js_clear_timeout_value", VOID, &[DOUBLE]);
    module.declare_function("js_clear_interval_value", VOID, &[DOUBLE]);
    module.declare_function("js_clear_immediate_value", VOID, &[DOUBLE]);
    module.declare_function("js_buffer_from_array", I64, &[I64]);
    module.declare_function("js_buffer_from_arraybuffer_slice", I64, &[I64, I32, I32]);
    module.declare_function("js_buffer_length", I32, &[I64]);
    module.declare_function("js_buffer_get", I32, &[I64, I32]);
    module.declare_function("js_native_buffer_data_ptr", PTR, &[DOUBLE]);
    module.declare_function("js_native_buffer_byte_len", I64, &[DOUBLE]);
    // console.time/count runtime functions.
    module.declare_function("js_console_time", VOID, &[I64]);
    module.declare_function("js_console_time_end", VOID, &[I64]);
    module.declare_function("js_console_time_log", VOID, &[I64]);
    module.declare_function("js_console_time_value", VOID, &[DOUBLE]);
    module.declare_function("js_console_time_end_value", VOID, &[DOUBLE]);
    module.declare_function("js_console_time_log_value", VOID, &[DOUBLE]);
    module.declare_function("js_console_time_log_spread", VOID, &[DOUBLE, I64]);
    module.declare_function("js_console_count", VOID, &[I64]);
    module.declare_function("js_console_count_reset", VOID, &[I64]);
    module.declare_function("js_console_count_value", VOID, &[DOUBLE]);
    module.declare_function("js_console_count_reset_value", VOID, &[DOUBLE]);
    module.declare_function("js_console_new", DOUBLE, &[DOUBLE]);
    module.declare_function("js_console_new2", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_console_group_begin", VOID, &[]);
    module.declare_function("js_console_group_end", VOID, &[]);
    module.declare_function("js_console_clear", VOID, &[]);
    module.declare_function("js_console_noop", VOID, &[]);
    // Universal PropertyGet method dispatch fallback — routes
    // `recv.method(args)` to the runtime's dispatcher when no static
    // codegen path fires. Used by Map/Set methods on plain object fields.
    module.declare_function(
        "js_native_call_method",
        DOUBLE,
        &[DOUBLE, PTR, I64, PTR, I64],
    );
    module.declare_function(
        "js_native_call_method_by_id",
        DOUBLE,
        &[DOUBLE, I64, PTR, I64],
    );
    // Apply form: takes the args as a JS array handle (i64). The runtime
    // materialises the array elements into a temp f64 buffer and forwards to
    // js_native_call_method. Used by `Expr::CallSpread` for the
    // `recv.method(...args)` shape on any-typed receivers.
    module.declare_function(
        "js_native_call_method_apply",
        DOUBLE,
        &[DOUBLE, PTR, I64, I64],
    );
    module.declare_function(
        "js_native_call_method_apply_by_id",
        DOUBLE,
        &[DOUBLE, I64, I64],
    );
    // v0.5.754: dispatch obj[strKey](args) — computed-key method call.
    // Takes a StringHeader pointer (already-unboxed) for the method name.
    module.declare_function(
        "js_native_call_method_str_key",
        DOUBLE,
        &[DOUBLE, I64, PTR, I64],
    );
    // #321: dispatch obj[key](args) for a runtime-value key (not statically a
    // string). Binds `this = obj` for any key type — string keys go through
    // the full dispatch tower, symbol/other keys read the property then call
    // with `this` bound. (object, key, args_ptr, args_len) -> result.
    module.declare_function(
        "js_native_call_method_value",
        DOUBLE,
        &[DOUBLE, DOUBLE, PTR, I64],
    );
    // Apply form of obj[key](...args): runtime-value key + args as a JS array
    // handle. Materialises the array and forwards to js_native_call_method_value
    // (binds `this = obj`). Used by `Expr::CallSpread` for the computed-member
    // `recv[prop](...args)` shape, which otherwise dropped `this`.
    module.declare_function(
        "js_native_call_method_value_apply",
        DOUBLE,
        &[DOUBLE, DOUBLE, I64],
    );
    module.declare_function("js_promise_resolve", VOID, &[I64, DOUBLE]);
    module.declare_function("js_promise_reject", VOID, &[I64, DOUBLE]);
    module.declare_function("js_promise_resolved", I64, &[DOUBLE]);
    module.declare_function("js_promise_rejected", I64, &[DOUBLE]);
    // Issue #100: build a module-namespace object from parallel key/
    // value arrays. Called from `__perry_init_<prefix>` (populate the
    // module's `__perry_ns_<prefix>` global) and from `Expr::DynamicImport`
    // (returned wrapped in `js_promise_resolved`). See
    // `crates/perry-runtime/src/object.rs::js_create_namespace`.
    module.declare_function("js_create_namespace", DOUBLE, &[I32, PTR, PTR, PTR]);
    module.declare_function("js_promise_then", I64, &[I64, I64, I64]);
    module.declare_function("js_promise_resolved_then", I64, &[DOUBLE, I64, I64]);
    module.declare_function("js_promise_finally", I64, &[I64, I64]);
    // #5849: own-property-aware `.then`/`.catch`/`.finally` entries for a
    // statically-known-Promise receiver — boxed f64 in, boxed f64 out (see
    // `lower_call/property_get/promise_chain.rs`).
    module.declare_function("js_promise_then_checked", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_promise_catch_checked", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_promise_finally_checked", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_promise_closure_arg", I64, &[DOUBLE]);
    module.declare_function("js_promise_all", I64, &[I64]);
    module.declare_function("js_promise_race", I64, &[I64]);
    module.declare_function("js_promise_any", I64, &[I64]);
    module.declare_function("js_promise_all_settled", I64, &[I64]);
    // #2822: iterable-accepting combinator entries (take a boxed f64 value,
    // coerce iterables to an array, reject non-iterables with TypeError).
    module.declare_function("js_promise_all_iterable", I64, &[DOUBLE]);
    module.declare_function("js_promise_race_iterable", I64, &[DOUBLE]);
    module.declare_function("js_promise_any_iterable", I64, &[DOUBLE]);
    module.declare_function("js_promise_all_settled_iterable", I64, &[DOUBLE]);
    module.declare_function("js_promise_with_resolvers", I64, &[]);
    module.declare_function("js_promise_try", I64, &[DOUBLE, I64]);
    module.declare_function("js_array_unshift_f64", I64, &[I64, DOUBLE]);
    // #2814: variadic unshift (insert N items at front, in order).
    module.declare_function("js_array_unshift_variadic", I64, &[I64, PTR, I32]);
    module.declare_function("js_array_entries", I64, &[I64]);
    module.declare_function("js_array_keys", I64, &[I64]);
    module.declare_function("js_array_values", I64, &[I64]);
    // #2384: iterator-OBJECT variants (real `.next()`-bearing iterator, not an
    // eager materialized array) backing codegen's `Expr::ArrayValues`/etc.
    module.declare_function("js_array_entries_iter_obj", I64, &[I64]);
    module.declare_function("js_array_keys_iter_obj", I64, &[I64]);
    module.declare_function("js_array_values_iter_obj", I64, &[I64]);

    // ──────────────────────────────────────────────────────────────────
    // Web Fetch API: Response / Headers / Request / Blob constructors +
    // body methods + static factories. These are in
    // `crates/perry-stdlib/src/fetch.rs`. Handles are NaN-boxed POINTER_TAG
    // f64 values (Phase 1 of the handle-NaN-boxing unification) — codegen
    // passes them through as DOUBLE arg kinds without conversion. Untyped
    // property access (`request.url` where `request: any`) routes through
    // `js_object_get_field_by_name`'s strip-tag path → `HANDLE_PROPERTY_DISPATCH`.
    // ──────────────────────────────────────────────────────────────────
    // new Response(body_ptr, status, status_text_ptr, headers_handle) -> f64
    module.declare_function("js_response_new", DOUBLE, &[I64, DOUBLE, I64, DOUBLE]);
    // js_response_body_init_ptr(body_value_f64) -> string_ptr (i64): drains a
    // ReadableStream body to bytes, else falls back to string coercion.
    module.declare_function("js_response_body_init_ptr", I64, &[DOUBLE]);
    // new Headers() -> f64
    module.declare_function("js_headers_new", DOUBLE, &[]);
    // headers.set(handle_f64, key_ptr, val_ptr) -> f64 (undefined-tag)
    module.declare_function("js_headers_set", DOUBLE, &[DOUBLE, I64, I64]);
    // headers.append(handle_f64, key_ptr, val_ptr) -> f64 (undefined-tag)
    module.declare_function("js_headers_append", DOUBLE, &[DOUBLE, I64, I64]);
    // headers.get(handle_f64, key_ptr) -> *mut StringHeader (i64)
    module.declare_function("js_headers_get", I64, &[DOUBLE, I64]);
    // headers.getSetCookie(handle_f64) -> f64 (NaN-boxed POINTER_TAG to ArrayHeader)
    module.declare_function("js_headers_get_set_cookie", DOUBLE, &[DOUBLE]);
    // headers.has(handle_f64, key_ptr) -> f64 (TAG_TRUE/FALSE)
    module.declare_function("js_headers_has", DOUBLE, &[DOUBLE, I64]);
    // headers.delete(handle_f64, key_ptr) -> f64 (undefined-tag)
    module.declare_function("js_headers_delete", DOUBLE, &[DOUBLE, I64]);
    // headers.forEach(handle_f64, cb_nanbox) -> f64 (undefined-tag)
    module.declare_function("js_headers_for_each", DOUBLE, &[DOUBLE, DOUBLE]);
    // headers.keys/values/entries(handle_f64) -> f64 (NaN-boxed POINTER_TAG to ArrayHeader)
    module.declare_function("js_headers_keys", DOUBLE, &[DOUBLE]);
    module.declare_function("js_headers_values", DOUBLE, &[DOUBLE]);
    module.declare_function("js_headers_entries", DOUBLE, &[DOUBLE]);
    module.declare_function("js_headers_init_from_value", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_headers_method_value", DOUBLE, &[DOUBLE, I64, I64]);

    // new Request(url_ptr, method_ptr, body_ptr, headers_handle_f64, metadata...) -> f64
    module.declare_function(
        "js_request_new",
        DOUBLE,
        &[
            I64, I64, I64, DOUBLE, I64, I64, I64, I64, I64, I64, I64, DOUBLE, I64, DOUBLE,
        ],
    );
    // #5458: new Request(url_ptr, init_value_f64) -> f64 — runtime-field
    // fallback for init shapes codegen can't statically extract (call results,
    // spreads, dynamic objects). Reads method/body/headers/... off the init
    // object at runtime instead of silently dropping them.
    module.declare_function("js_request_new_from_init", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_request_get_url", I64, &[DOUBLE]);
    module.declare_function("js_request_get_method", I64, &[DOUBLE]);
    module.declare_function("js_request_get_body", DOUBLE, &[DOUBLE]);
    module.declare_function("js_request_body_used", DOUBLE, &[DOUBLE]);
    module.declare_function("js_request_get_destination", I64, &[DOUBLE]);
    module.declare_function("js_request_get_referrer", I64, &[DOUBLE]);
    module.declare_function("js_request_get_referrer_policy", I64, &[DOUBLE]);
    module.declare_function("js_request_get_mode", I64, &[DOUBLE]);
    module.declare_function("js_request_get_credentials", I64, &[DOUBLE]);
    module.declare_function("js_request_get_cache", I64, &[DOUBLE]);
    module.declare_function("js_request_get_redirect", I64, &[DOUBLE]);
    module.declare_function("js_request_get_integrity", I64, &[DOUBLE]);
    module.declare_function("js_request_get_keepalive", DOUBLE, &[DOUBLE]);
    module.declare_function("js_request_get_duplex", I64, &[DOUBLE]);
    module.declare_function("js_request_get_signal", DOUBLE, &[DOUBLE]);
    // #1649: `req.headers` → NaN-boxed Headers handle.
    module.declare_function("js_request_get_headers", DOUBLE, &[DOUBLE]);
    // #1688: request body-consuming methods. text/json/arrayBuffer return a
    // Promise pointer (i64); codegen NaN-boxes it as POINTER_TAG.
    module.declare_function("js_request_text", I64, &[DOUBLE]);
    module.declare_function("js_request_json", I64, &[DOUBLE]);
    module.declare_function("js_request_array_buffer", I64, &[DOUBLE]);
    module.declare_function("js_request_blob", I64, &[DOUBLE]);
    module.declare_function("js_request_bytes", I64, &[DOUBLE]);
    module.declare_function("js_request_form_data", I64, &[DOUBLE]);
    module.declare_function("js_request_clone", DOUBLE, &[DOUBLE]);

    // Response body getters — handles flow as NaN-boxed POINTER_TAG f64
    // (Phase 1 unification, refs #421). Accessors call `handle_id` to
    // unbox on entry; codegen no longer needs the fptosi conversion.
    module.declare_function("js_fetch_response_status", DOUBLE, &[DOUBLE]);
    module.declare_function("js_fetch_response_status_text", I64, &[DOUBLE]);
    module.declare_function("js_fetch_response_ok", DOUBLE, &[DOUBLE]);
    module.declare_function("js_fetch_response_type", I64, &[DOUBLE]);
    module.declare_function("js_fetch_response_url", I64, &[DOUBLE]);
    module.declare_function("js_fetch_response_redirected", DOUBLE, &[DOUBLE]);
    module.declare_function("js_response_body_used", DOUBLE, &[DOUBLE]);
    module.declare_function("js_fetch_response_text", I64, &[DOUBLE]);
    module.declare_function("js_fetch_response_json", I64, &[DOUBLE]);
    // response.headers / .clone() / .arrayBuffer() / .blob() — all take
    // the f64 response handle.
    module.declare_function("js_response_get_headers", DOUBLE, &[DOUBLE]);
    module.declare_function("js_response_clone", DOUBLE, &[DOUBLE]);
    module.declare_function("js_response_array_buffer", I64, &[DOUBLE]);
    module.declare_function("js_response_blob", I64, &[DOUBLE]);
    module.declare_function("js_response_bytes", I64, &[DOUBLE]);
    module.declare_function("js_response_form_data", I64, &[DOUBLE]);
    module.declare_function("js_form_data_new", DOUBLE, &[]);
    module.declare_function("js_form_data_append", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_form_data_set", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_form_data_delete", DOUBLE, &[DOUBLE, I64]);
    module.declare_function("js_form_data_get", DOUBLE, &[DOUBLE, I64]);
    module.declare_function("js_form_data_get_all", DOUBLE, &[DOUBLE, I64]);
    module.declare_function("js_form_data_has", DOUBLE, &[DOUBLE, I64]);
    module.declare_function("js_form_data_entries", DOUBLE, &[DOUBLE]);
    module.declare_function("js_form_data_keys", DOUBLE, &[DOUBLE]);
    module.declare_function("js_form_data_values", DOUBLE, &[DOUBLE]);
    module.declare_function("js_form_data_for_each", DOUBLE, &[DOUBLE, DOUBLE]);
    // Blob instance methods (issue #234) — handle is f64 (registry id).
    // arrayBuffer/bytes/text return a Promise pointer (i64); slice returns a
    // new blob handle as f64.
    module.declare_function("js_blob_size", DOUBLE, &[DOUBLE]);
    module.declare_function("js_blob_type", I64, &[DOUBLE]);
    module.declare_function("js_blob_array_buffer", I64, &[DOUBLE]);
    module.declare_function("js_blob_bytes", I64, &[DOUBLE]);
    module.declare_function("js_blob_text", I64, &[DOUBLE]);
    module.declare_function("js_blob_slice", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE, I64]);
    // Issue #1211: Blob / File constructors + object-URL registry.
    module.declare_function("js_blob_new", DOUBLE, &[DOUBLE, DOUBLE]);
    module.declare_function("js_file_new", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_file_name", I64, &[DOUBLE]);
    module.declare_function("js_file_last_modified", DOUBLE, &[DOUBLE]);
    module.declare_function("js_url_create_object_url", I64, &[DOUBLE]);
    module.declare_function("js_url_revoke_object_url", VOID, &[DOUBLE]);
    module.declare_function("js_buffer_resolve_object_url", DOUBLE, &[DOUBLE]);
    // Static factories.
    module.declare_function(
        "js_response_static_json",
        DOUBLE,
        &[DOUBLE, DOUBLE, I64, DOUBLE],
    );
    module.declare_function("js_response_static_redirect", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_response_static_error", DOUBLE, &[]);

    // ──────────────────────────────────────────────────────────────────
    // Web Streams API (issue #237) — perry-stdlib/src/streams.rs +
    // blob.stream() / response.body bridge in perry-stdlib/src/fetch.rs.
    // Handles are numeric registry ids carried as f64; promise-returning
    // FFIs return *mut Promise (I64) which codegen NaN-boxes via
    // nanbox_pointer_inline.
    // ──────────────────────────────────────────────────────────────────
    module.declare_function("js_blob_stream", DOUBLE, &[DOUBLE]);
    module.declare_function("js_response_body", DOUBLE, &[DOUBLE]);
    // ReadableStream constructor + methods.
    module.declare_function(
        "js_readable_stream_new",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_readable_stream_new_with_source_type",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_readable_stream_new_with_strategy_and_source_type",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_readable_stream_new_from_source_object",
        DOUBLE,
        &[DOUBLE, DOUBLE],
    );
    module.declare_function("js_readable_stream_get_reader", DOUBLE, &[DOUBLE]);
    module.declare_function(
        "js_readable_stream_get_reader_with_options",
        DOUBLE,
        &[DOUBLE, DOUBLE],
    );
    // #4915: BYOB reader surface.
    module.declare_function("js_readable_stream_get_byob_reader", DOUBLE, &[DOUBLE]);
    module.declare_function(
        "js_readable_stream_controller_byob_request",
        DOUBLE,
        &[DOUBLE],
    );
    // #1645: ReadableStream.from(iterable) — builds a pre-loaded stream.
    module.declare_function("js_readable_stream_from_iterable", DOUBLE, &[DOUBLE]);
    module.declare_function("js_readable_stream_locked", DOUBLE, &[DOUBLE]);
    module.declare_function("js_readable_stream_cancel", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_readable_stream_tee", DOUBLE, &[DOUBLE]);
    module.declare_function("js_readable_stream_pipe_to", I64, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function(
        "js_readable_stream_pipe_through",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_readable_stream_controller_enqueue",
        DOUBLE,
        &[DOUBLE, DOUBLE],
    );
    module.declare_function("js_readable_stream_controller_close", DOUBLE, &[DOUBLE]);
    module.declare_function(
        "js_readable_stream_controller_error",
        DOUBLE,
        &[DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_readable_stream_controller_desired_size",
        DOUBLE,
        &[DOUBLE],
    );
    // ReadableStreamDefaultReader.
    module.declare_function("js_reader_read", I64, &[DOUBLE]);
    // #4915: BYOB `read(view)` — fills the caller-supplied view.
    module.declare_function("js_reader_read_with_view", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_reader_release_lock", DOUBLE, &[DOUBLE]);
    module.declare_function("js_reader_closed", I64, &[DOUBLE]);
    module.declare_function("js_reader_cancel", I64, &[DOUBLE, DOUBLE]);
    // WritableStream + Writer. #1545: leading arg is the `start` hook.
    module.declare_function(
        "js_writable_stream_new",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_writable_stream_new_with_sink_type",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_writable_stream_new_from_sink_object",
        DOUBLE,
        &[DOUBLE, DOUBLE],
    );
    module.declare_function("js_writable_stream_throw_invalid_sink", DOUBLE, &[]);
    module.declare_function("js_writable_stream_get_writer", DOUBLE, &[DOUBLE]);
    module.declare_function("js_writable_stream_locked", DOUBLE, &[DOUBLE]);
    module.declare_function("js_writable_stream_close", I64, &[DOUBLE]);
    module.declare_function("js_writable_stream_abort", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_writer_write", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_writer_close", I64, &[DOUBLE]);
    module.declare_function("js_writer_abort", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_writer_release_lock", DOUBLE, &[DOUBLE]);
    module.declare_function("js_writer_closed", I64, &[DOUBLE]);
    module.declare_function("js_writer_ready", I64, &[DOUBLE]);
    module.declare_function("js_writer_desired_size", DOUBLE, &[DOUBLE]);
    // TransformStream.
    // #1644: leading arg is the `start` hook. #4915: the two trailing args
    // are the writable / readable strategies (number or strategy object).
    module.declare_function(
        "js_transform_stream_new",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_transform_stream_new_from_transformer_object",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function("js_transform_stream_readable", DOUBLE, &[DOUBLE]);
    module.declare_function("js_transform_stream_writable", DOUBLE, &[DOUBLE]);
    module.declare_function("js_text_encoding_stream_new", DOUBLE, &[]);
    module.declare_function("js_text_encoder_stream_new", DOUBLE, &[]);
    module.declare_function("js_text_decoder_stream_new", DOUBLE, &[]);
    module.declare_function("js_stream_web_text_encoder_stream_new", DOUBLE, &[]);
    module.declare_function(
        "js_stream_web_text_decoder_stream_new",
        DOUBLE,
        &[DOUBLE, DOUBLE],
    );
    module.declare_function("js_stream_web_compression_stream_new", DOUBLE, &[DOUBLE]);
    module.declare_function("js_stream_web_decompression_stream_new", DOUBLE, &[DOUBLE]);
    // #1545: node:stream/web QueuingStrategy constructors — take the options
    // object, return a `{ highWaterMark, size }` object.
    module.declare_function("js_streams_strategy_high_water_mark", DOUBLE, &[DOUBLE]);
    module.declare_function("js_count_queuing_strategy_new", DOUBLE, &[DOUBLE]);
    module.declare_function("js_byte_length_queuing_strategy_new", DOUBLE, &[DOUBLE]);
    // Issue #562: stream subclassing (`class X extends WritableStream` etc.).
    // The unwrap helper is wrapped around every stream-FFI receiver so a
    // subclass instance (NaN-boxed object pointer with the registry id
    // stashed under `__perry_stream_handle__`) and a bare numeric
    // handle are interchangeable. The `*_subclass_init` shims are
    // invoked from `Expr::SuperCall` codegen for the three Web Stream
    // base classes.
    module.declare_function("js_stream_unwrap_handle", DOUBLE, &[DOUBLE]);
    module.declare_function(
        "js_readable_stream_subclass_init",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_writable_stream_subclass_init",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_transform_stream_subclass_init",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    // `class X extends Request` / `extends Response`: `super(...)` shims that
    // allocate the underlying native fetch handle and stash it on `this`.
    // Invoked from the `Expr::SuperCall` Request/Response arm.
    module.declare_function(
        "js_request_subclass_init",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function(
        "js_response_subclass_init",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE],
    );
    // Runtime-value super() dispatcher: identifies an aliased global
    // Request/Response constructor parent (e.g. `extends GlobalRequest`) and
    // stashes the native handle on `this`, else forwards to js_native_call_value.
    module.declare_function(
        "js_fetch_or_value_super",
        DOUBLE,
        &[DOUBLE, DOUBLE, PTR, I64],
    );

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
    // #2582: AbortSignal static helpers + lifecycle.
    module.declare_function("js_abort_signal_abort", I64, &[DOUBLE]);
    module.declare_function("js_abort_signal_any", I64, &[I64]);
    module.declare_function("js_abort_signal_throw_if_aborted", DOUBLE, &[I64]);
    module.declare_function("js_event_target_new", I64, &[]);
    module.declare_function("js_event_new", I64, &[DOUBLE, DOUBLE, I32]);
    // `super(type, options)` from `class X extends Event/CustomEvent` —
    // initializes Event fields onto the existing subclass `this`.
    module.declare_function(
        "js_event_subclass_init",
        DOUBLE,
        &[DOUBLE, DOUBLE, DOUBLE, I32, I32],
    );
    module.declare_function("js_custom_event_new", I64, &[DOUBLE, DOUBLE, I32]);
    module.declare_function("js_dom_exception_new", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_event_target_add_event_listener", VOID, &[I64, I64, I64]);
    module.declare_function(
        "js_event_target_add_event_listener_with_options",
        VOID,
        &[I64, I64, I64, DOUBLE],
    );
    module.declare_function(
        "js_event_target_remove_event_listener",
        VOID,
        &[I64, I64, I64],
    );
    module.declare_function(
        "js_event_target_remove_event_listener_with_options",
        VOID,
        &[I64, I64, I64, DOUBLE],
    );
    module.declare_function("js_event_target_dispatch_event", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_event_target_is_event_target", I32, &[I64]);
    module.declare_function("js_event_target_get_event_listeners", I64, &[I64, I64]);
    module.declare_function("js_event_target_get_max_listeners", DOUBLE, &[I64]);
    module.declare_function("js_event_target_set_max_listeners", I32, &[I64, DOUBLE]);
    module.declare_function("js_message_channel_new", DOUBLE, &[]);
    module.declare_function("js_message_port_constructor_error", DOUBLE, &[]);
    module.declare_function("js_broadcast_channel_new", DOUBLE, &[DOUBLE]);
}
