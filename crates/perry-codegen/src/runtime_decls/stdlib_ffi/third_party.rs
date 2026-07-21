//! Third-party package stdlib FFI declarations (extracted from stdlib_ffi.rs):
//! bcrypt/argon2, perry/ads, perry/thread, jsonwebtoken, axios, sharp, cron,
//! async_hooks/AsyncLocalStorage, DisposableStack, zlib, Buffer, child_process,
//! cheerio.

use crate::module::LlModule;
use crate::types::{DOUBLE, I32, I64, PTR, VOID};

pub(crate) fn declare_third_party(module: &mut LlModule) {
    // ========== bcrypt / argon2 ==========
    module.declare_function("js_argon2_hash", I64, &[I64]);
    module.declare_function("js_argon2_hash_options", I64, &[I64, I64]);
    module.declare_function("js_argon2_verify", I64, &[I64, I64]);
    module.declare_function("js_bcrypt_compare", I64, &[I64, I64]);
    module.declare_function("js_bcrypt_compare_sync", DOUBLE, &[I64, I64]);
    module.declare_function("js_bcrypt_gen_salt", I64, &[DOUBLE]);
    module.declare_function("js_bcrypt_hash", I64, &[I64, DOUBLE]);
    module.declare_function("js_bcrypt_hash_sync", I64, &[I64, DOUBLE]);

    // `@perryts/google-auth` is no longer declared centrally — the
    // signatures come from the installed npm package's
    // `perry.nativeLibrary.functions` block (see
    // https://github.com/PerryTS/google-auth) and are added to
    // `ffi_signatures` on demand by the external-nativeLibrary path.

    // ========== perry/ads (issue #867) ==========
    // Four promise-returning entry points (NR_PTR — i64 return,
    // NaN-boxed as POINTER) plus two synchronous banner FFI
    // functions (NR_F64 / NR_VOID). String args lower to
    // `*const StringHeader` (i64) per the codegen NA_STR
    // convention; the f64 handle is the NaN-boxable numeric
    // return for banner_create.
    module.declare_function("js_ads_interstitial_load", I64, &[I64]);
    module.declare_function("js_ads_interstitial_show", I64, &[]);
    module.declare_function("js_ads_rewarded_load", I64, &[I64]);
    module.declare_function("js_ads_rewarded_show", I64, &[]);
    module.declare_function("js_ads_banner_create", DOUBLE, &[I64, I64]);
    module.declare_function("js_ads_banner_destroy", VOID, &[DOUBLE]);
    module.declare_function("js_ads_request_consent", I64, &[]);

    // ========== perry embedded-asset API (#5731) ==========
    // readEmbedded(path) → *mut BufferHeader (returned as I64 pointer).
    module.declare_function("js_perry_read_embedded", I64, &[DOUBLE]);
    // embeddedFiles() → *mut ArrayHeader (returned as I64 pointer).
    module.declare_function("js_perry_embedded_files", I64, &[]);

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
    module.declare_function("js_jwt_verify_es256", I64, &[I64, I64]);
    module.declare_function("js_jwt_verify_rs256", I64, &[I64, I64]);
    // #1074: runtime-algorithm dispatchers. The codegen `lower_jsonwebtoken_*`
    // fast paths still hard-route literal `algorithm: "ES256"` to the typed
    // helpers above; non-literal shapes (const-bound ident, spread, ternary)
    // are routed here with the alg name lowered as a string at runtime.
    module.declare_function("js_jwt_sign_dyn", I64, &[I64, I64, I64, DOUBLE, I64]);
    module.declare_function("js_jwt_verify_dyn", I64, &[I64, I64, I64]);
    // #1074 case C: options is a whole non-extractable expression
    // (`const opts = { algorithm: "ES256" }; jwt.sign(p, k, opts)`). We
    // pass `opts` as a NaN-boxed JSValue and the runtime helper extracts
    // `algorithm` / `expiresIn` / `keyid` via `js_object_get_field_by_name`.
    module.declare_function("js_jwt_sign_dyn_opts", I64, &[I64, I64, DOUBLE]);
    module.declare_function("js_jwt_verify_dyn_opts", I64, &[I64, I64, DOUBLE]);

    // ========== axios / node-fetch ==========
    module.declare_function("js_axios_create", DOUBLE, &[I64]);
    module.declare_function("js_axios_delete", I64, &[I64]);
    module.declare_function("js_axios_get", I64, &[I64]);
    // #598: body arg is a NaN-boxed f64 (DOUBLE) so the runtime can
    // distinguish strings from objects via the tag and JSON.stringify
    // non-string bodies. Pre-fix this was I64 (raw unboxed pointer)
    // which had no way to tell `axios.post(url, "raw json")` from
    // `axios.post(url, {a: 1})`.
    module.declare_function("js_axios_post", I64, &[I64, DOUBLE]);
    module.declare_function("js_axios_put", I64, &[I64, DOUBLE]);
    module.declare_function("js_axios_patch", I64, &[I64, DOUBLE]);
    module.declare_function("js_axios_request", I64, &[I64]);
    module.declare_function("js_axios_response_status", DOUBLE, &[I64]);
    module.declare_function("js_axios_response_status_text", I64, &[I64]);
    module.declare_function("js_axios_response_data", I64, &[I64]);
    // Issue #604 followup — JSON-auto-parsing variant of `.data`. Returns
    // a NaN-boxed JSValue (parsed object/array/number/bool/null when the
    // response body is JSON, raw string otherwise) so `r.data.ok` works
    // the same way as npm `axios` does for `application/json` responses.
    module.declare_function("js_axios_response_data_parsed", DOUBLE, &[I64]);

    // ========== sharp / image ==========
    module.declare_function("js_sharp_auto_orient", I64, &[I64]);
    module.declare_function("js_sharp_avif", I64, &[I64, DOUBLE]);
    module.declare_function("js_sharp_blur", I64, &[I64, DOUBLE]);
    module.declare_function("js_sharp_composite", I64, &[I64, DOUBLE]);
    module.declare_function("js_sharp_extend", I64, &[I64, DOUBLE]);
    module.declare_function("js_sharp_extract", I64, &[I64, DOUBLE]);
    module.declare_function("js_sharp_flip", I64, &[I64]);
    module.declare_function("js_sharp_flop", I64, &[I64]);
    module.declare_function("js_sharp_from_buffer", I64, &[I64, DOUBLE]);
    module.declare_function("js_sharp_from_file", I64, &[I64]);
    module.declare_function("js_sharp_from_input", I64, &[I64]);
    module.declare_function("js_sharp_grayscale", I64, &[I64]);
    module.declare_function("js_sharp_metadata", I64, &[I64]);
    module.declare_function("js_sharp_sharpen", I64, &[I64]);
    module.declare_function("js_sharp_trim", I64, &[I64]);
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
    // npm `cron` CronJob ctor arm in lower_call/builtin.rs —
    // (expr StringHeader, onTick closure bits, NaN-boxed start flag).
    module.declare_function("js_cron_job_new", I64, &[I64, I64, DOUBLE]);
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
    module.declare_function("js_async_hooks_create_hook", I64, &[DOUBLE]);
    module.declare_function("js_async_hooks_execution_async_id", DOUBLE, &[]);
    module.declare_function("js_async_hooks_trigger_async_id", DOUBLE, &[]);
    module.declare_function("js_async_hooks_execution_async_resource", DOUBLE, &[]);
    module.declare_function("js_async_hook_enable", I64, &[I64]);
    module.declare_function("js_async_hook_disable", I64, &[I64]);
    module.declare_function("js_async_resource_new", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_async_resource_async_id", DOUBLE, &[I64]);
    module.declare_function("js_async_resource_trigger_async_id", DOUBLE, &[I64]);
    module.declare_function("js_async_resource_emit_destroy", I64, &[I64]);
    module.declare_function(
        "js_async_resource_run_in_async_scope",
        DOUBLE,
        &[I64, DOUBLE, DOUBLE, I64],
    );
    module.declare_function("js_async_resource_bind", I64, &[I64, DOUBLE, DOUBLE]);
    module.declare_function("js_async_resource_static_bind", I64, &[I64, DOUBLE]);
    module.declare_function("js_async_local_storage_disable", VOID, &[I64]);
    module.declare_function("js_async_local_storage_enter_with", VOID, &[I64, DOUBLE]);
    // #3092 — callback is passed as a full NaN-boxed value (DOUBLE), not a raw
    // pointer, so the runtime can reject non-callable callbacks.
    module.declare_function("js_async_local_storage_exit", DOUBLE, &[I64, DOUBLE, I64]);
    module.declare_function("js_async_local_storage_get_store", DOUBLE, &[I64]);
    module.declare_function("js_async_local_storage_new", I64, &[]);
    module.declare_function(
        "js_async_local_storage_run",
        DOUBLE,
        &[I64, DOUBLE, DOUBLE, I64],
    );

    // ========== #2875 DisposableStack / AsyncDisposableStack / SuppressedError ==========
    // `new` ctors (dispatched by lower_builtin_new). Instance methods are
    // declared through the native_table dispatch path, but the constructors
    // are called directly so they need an explicit declaration here.
    module.declare_function("js_disposable_stack_new", I64, &[]);
    module.declare_function("js_async_disposable_stack_new", I64, &[]);
    module.declare_function("js_suppressed_error_new", DOUBLE, &[DOUBLE, DOUBLE, DOUBLE]);

    // ========== zlib ==========
    // #2935: gzipSync/deflateSync take the data as raw NaN-box bits (I64) plus
    // an options object (DOUBLE) so the `{ level }` option can select the
    // compression level / throw RangeError. The codec unboxes the data itself.
    module.declare_function("js_zlib_deflate_sync", I64, &[I64, DOUBLE]);
    module.declare_function("js_zlib_deflate", VOID, &[DOUBLE, DOUBLE]);
    module.declare_function("js_zlib_gunzip_sync", I64, &[I64]);
    module.declare_function("js_zlib_gunzip", VOID, &[DOUBLE, DOUBLE]);
    module.declare_function("js_zlib_gzip_sync", I64, &[I64, DOUBLE]);
    module.declare_function("js_zlib_gzip", VOID, &[DOUBLE, DOUBLE]);
    module.declare_function("js_zlib_inflate_sync", I64, &[I64]);
    module.declare_function("js_zlib_inflate", VOID, &[DOUBLE, DOUBLE]);
    module.declare_function("js_zlib_deflate_raw_sync", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_zlib_deflate_raw", VOID, &[DOUBLE, DOUBLE]);
    module.declare_function("js_zlib_inflate_raw_sync", I64, &[DOUBLE]);
    module.declare_function("js_zlib_inflate_raw", VOID, &[DOUBLE, DOUBLE]);
    module.declare_function("js_zlib_unzip_sync", I64, &[DOUBLE]);
    module.declare_function("js_zlib_unzip", VOID, &[DOUBLE, DOUBLE]);
    module.declare_function("js_zlib_crc32", DOUBLE, &[DOUBLE, DOUBLE]);
    // Brotli sync one-shots take data as raw NaN-box bits for the same
    // shared validation path as gzipSync/deflateSync.
    module.declare_function("js_zlib_brotli_compress_sync", I64, &[I64]);
    module.declare_function("js_zlib_brotli_decompress_sync", I64, &[I64]);
    module.declare_function("js_zlib_brotli_compress", VOID, &[DOUBLE, DOUBLE]);
    module.declare_function("js_zlib_brotli_decompress", VOID, &[DOUBLE, DOUBLE]);
    module.declare_function("js_zlib_zstd_compress_sync", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_zlib_zstd_decompress_sync", I64, &[DOUBLE, DOUBLE]);
    module.declare_function("js_zlib_zstd_compress", VOID, &[DOUBLE, DOUBLE]);
    module.declare_function("js_zlib_zstd_decompress", VOID, &[DOUBLE, DOUBLE]);
    // #1843 — Transform-stream factories: `_opts` (DOUBLE) in, i64 handle out.
    // (`js_zlib_create_brotli_decompress` is declared alongside the other
    // crypto/zlib helpers in runtime_decls/strings.rs.)
    module.declare_function("js_zlib_create_gzip", I64, &[DOUBLE]);
    module.declare_function("js_zlib_create_gunzip", I64, &[DOUBLE]);
    module.declare_function("js_zlib_create_deflate", I64, &[DOUBLE]);
    module.declare_function("js_zlib_create_inflate", I64, &[DOUBLE]);
    module.declare_function("js_zlib_create_deflate_raw", I64, &[DOUBLE]);
    module.declare_function("js_zlib_create_inflate_raw", I64, &[DOUBLE]);
    module.declare_function("js_zlib_create_unzip", I64, &[DOUBLE]);
    module.declare_function("js_zlib_create_brotli_compress", I64, &[DOUBLE]);
    module.declare_function("js_zlib_create_zstd_compress", I64, &[DOUBLE]);
    module.declare_function("js_zlib_create_zstd_decompress", I64, &[DOUBLE]);

    // ========== Buffer ==========
    module.declare_function("js_buffer_alloc_unsafe", I64, &[I32]);
    module.declare_function("js_buffer_byte_length", I32, &[I64]);
    module.declare_function("js_buffer_byte_length_value", I32, &[DOUBLE, DOUBLE]);
    module.declare_function("js_buffer_concat", I64, &[I64]);
    module.declare_function("js_buffer_concat_with_length", I64, &[I64, DOUBLE]);
    // #2013: Node argument validation for the Buffer factory methods.
    module.declare_function("js_buffer_validate_size", I32, &[DOUBLE]);
    module.declare_function("js_buffer_validate_concat_list", I64, &[DOUBLE]);
    module.declare_function("js_buffer_copy", I32, &[I64, I64, I32, I32, I32]);
    module.declare_function("js_buffer_equals", I32, &[I64, I64]);
    module.declare_function("js_buffer_fill", I64, &[I64, I32]);
    module.declare_function("js_buffer_from_value", I64, &[I64, I32]);
    module.declare_function("js_buffer_is_ascii", DOUBLE, &[DOUBLE]);
    module.declare_function("js_buffer_is_buffer", I32, &[I64]);
    module.declare_function("js_buffer_is_encoding", I32, &[DOUBLE]);
    module.declare_function("js_buffer_is_utf8", DOUBLE, &[DOUBLE]);
    module.declare_function("js_buffer_print", VOID, &[I64]);
    module.declare_function("js_buffer_set", VOID, &[I64, I32, I32]);
    module.declare_function("js_buffer_set_from", VOID, &[I64, I64, I32]);
    module.declare_function("js_buffer_slice", I64, &[I64, I32, I32]);
    module.declare_function("js_buffer_to_string", I64, &[I64, I32]);
    // Issue #1210: `buffer.transcode(source, fromEnc, toEnc)`. Source is a
    // NaN-boxed Buffer pointer (DOUBLE), encodings are NaN-boxed strings
    // (DOUBLE). Returns a raw *mut BufferHeader (I64) — NR_PTR in the
    // native dispatch table NaN-boxes the result with POINTER_TAG.
    module.declare_function("js_buffer_transcode", I64, &[DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function("js_buffer_write", I32, &[I64, I64, I32, I32]);

    // ========== child_process ==========
    // execSync → NaN-boxed stdout (Buffer by default / string with `encoding`);
    // throws on non-zero exit. Returns DOUBLE. #1937/#1938.
    module.declare_function("js_child_process_exec_sync", DOUBLE, &[I64, I64]);
    // exec(cmd, options?, callback?): cmd string ptr (I64), options + callback
    // as NaN-boxed f64 in either slot; returns undefined (callback form) or the
    // stdout string (no-callback form). See `js_child_process_exec`.
    module.declare_function("js_child_process_exec", DOUBLE, &[I64, DOUBLE, DOUBLE]);
    module.declare_function("js_child_process_get_process_status", I64, &[DOUBLE]);
    module.declare_function("js_child_process_kill_process", I32, &[DOUBLE]);
    module.declare_function(
        "js_child_process_spawn_background",
        I64,
        &[DOUBLE, I64, DOUBLE, DOUBLE],
    );
    module.declare_function("js_child_process_spawn_sync", I64, &[I64, I64, I64]);
    // #1780: streaming spawn → NaN-boxed ChildProcess pointer (returns DOUBLE).
    module.declare_function("js_child_process_spawn_streams", DOUBLE, &[I64, I64, I64]);
    // #6563: node-pty spawn(file, args, options) — all three slots are raw
    // NaN-box bits (NA_JSV); returns the NaN-boxed IPty object.
    module.declare_function("js_pty_spawn", DOUBLE, &[I64, I64, I64]);
    // #1933: fork(modulePath, args, options) → NaN-boxed ChildProcess with an
    // IPC channel (send/disconnect/'message'/connected/channel).
    module.declare_function("js_child_process_fork", DOUBLE, &[I64, I64, I64]);
    // #1780: execFile (file, args, options, callback) + execFileSync (file, args, options).
    module.declare_function(
        "js_child_process_exec_file",
        DOUBLE,
        &[I64, DOUBLE, DOUBLE, DOUBLE],
    );
    // execFileSync → NaN-boxed stdout (Buffer by default / string with
    // `encoding`); throws on non-zero exit. Returns DOUBLE. #1937/#1938.
    module.declare_function(
        "js_child_process_exec_file_sync",
        DOUBLE,
        &[I64, DOUBLE, DOUBLE],
    );
    // #3079: setup-time command/file/args validation. The validators receive
    // the *original* NaN-boxed value (codegen still has it before unboxing to a
    // raw pointer) and throw `TypeError [ERR_INVALID_ARG_TYPE]` on a bad shape.
    // `validate_command` takes (value, name_ptr, name_len); `validate_args`
    // takes (value). Both return the value so the call can sit inline.
    module.declare_function(
        "js_child_process_validate_command",
        DOUBLE,
        &[DOUBLE, PTR, I32],
    );
    module.declare_function("js_child_process_validate_args", DOUBLE, &[DOUBLE]);

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
}
