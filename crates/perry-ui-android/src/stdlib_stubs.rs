//! No-op stubs for perry-stdlib symbols.
//! perry-stdlib is not bundled into libperry_ui_android.a; this file exists so
//! the linker can resolve `js_*` references emitted unconditionally by
//! codegen for stdlib-dependent FFIs that the app doesn't actually call.
//! Anything that needs a real Android impl gets force-linked from a smaller
//! standalone crate via `extern crate` in lib.rs (see `extern crate
//! perry_ext_sharp` for the issue #552 image-compression path).

// Symbols now provided by perry-runtime (removed from here to avoid duplicates):
// js_stdlib_init_dispatch, js_stdlib_process_pending (perry-runtime/stdlib_stubs.rs)
// js_array_unshift_jsvalue (perry-runtime/array.rs)
// js_await_any_promise (perry-runtime/promise.rs)
// js_ws_* (perry-runtime/stdlib_stubs.rs)
// js_json_* (perry-runtime/json.rs)

#[no_mangle]
pub extern "C" fn js_argon2_hash() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_argon2_hash_options() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_argon2_verify() -> i64 {
    0
}
// js_array_unshift_jsvalue — now provided by perry-runtime
#[no_mangle]
pub extern "C" fn js_async_local_storage_disable() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_async_local_storage_enter_with() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_async_local_storage_exit() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_async_local_storage_get_store() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_async_local_storage_new() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_async_local_storage_run() -> i64 {
    0
}
// js_await_any_promise — now provided by perry-runtime
#[no_mangle]
pub extern "C" fn js_await_js_promise() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_axios_create() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_axios_delete() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_axios_get() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_axios_post() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_axios_put() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_axios_request() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_bcrypt_compare() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_bcrypt_compare_sync() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_bcrypt_gen_salt() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_bcrypt_hash() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_bcrypt_hash_sync() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_cheerio_load() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_cheerio_load_fragment() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_cheerio_select() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_cheerio_selection_attr() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_cheerio_selection_attrs() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_cheerio_selection_children() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_cheerio_selection_eq() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_cheerio_selection_find() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_cheerio_selection_first() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_cheerio_selection_has_class() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_cheerio_selection_html() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_cheerio_selection_is() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_cheerio_selection_last() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_cheerio_selection_length() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_cheerio_selection_parent() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_cheerio_selection_text() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_cheerio_selection_texts() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_cheerio_selection_to_array() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_commander_action() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_commander_command() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_commander_description() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_commander_get_option() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_commander_get_option_bool() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_commander_get_option_number() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_commander_name() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_commander_new() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_commander_option() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_commander_opts() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_commander_parse() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_commander_required_option() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_commander_version() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_create_callback() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_cron_clear_interval() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_cron_clear_timeout() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_cron_describe() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_cron_job_is_running() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_cron_job_start() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_cron_job_stop() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_cron_next_date() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_cron_next_dates() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_cron_schedule() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_cron_set_interval() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_cron_set_timeout() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_cron_validate() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_cron_timer_tick() -> i32 {
    0
}
#[no_mangle]
pub extern "C" fn js_cron_timer_has_pending() -> i32 {
    0
}
#[no_mangle]
pub extern "C" fn js_crypto_aes256_decrypt() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_crypto_aes256_encrypt() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_crypto_hmac_sha256() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_crypto_md5() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_crypto_pbkdf2() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_crypto_random_bytes_hex() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_crypto_random_uuid() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_crypto_random_uuidv7() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_crypto_scrypt() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_crypto_scrypt_custom() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_crypto_sha256() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_datefns_add_days() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_datefns_add_months() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_datefns_add_years() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_datefns_difference_in_days() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_datefns_difference_in_hours() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_datefns_difference_in_minutes() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_datefns_end_of_day() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_datefns_format() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_datefns_is_after() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_datefns_is_before() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_datefns_parse_iso() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_datefns_start_of_day() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_dayjs_add() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_dayjs_date() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_dayjs_day() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_dayjs_diff() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_dayjs_end_of() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_dayjs_format() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_dayjs_from_timestamp() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_dayjs_hour() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_dayjs_is_after() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_dayjs_is_before() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_dayjs_is_same() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_dayjs_is_valid() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_dayjs_millisecond() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_dayjs_minute() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_dayjs_month() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_dayjs_now() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_dayjs_parse() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_dayjs_second() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_dayjs_start_of() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_dayjs_subtract() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_dayjs_to_iso_string() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_dayjs_unix() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_dayjs_value_of() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_dayjs_year() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_decimal_abs() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_decimal_div() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_decimal_eq() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_decimal_from_number() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_decimal_from_string() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_decimal_gt() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_decimal_lt() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_decimal_minus() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_decimal_plus() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_decimal_plus_number() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_decimal_sqrt() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_decimal_times() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_decimal_to_fixed() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_decimal_to_number() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_decimal_to_string() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_dotenv_config() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_dotenv_config_path() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_dotenv_parse() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_ethers_format_ether() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_ethers_format_units() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_ethers_get_address() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_ethers_parse_ether() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_ethers_parse_units() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_event_emitter_emit() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_event_emitter_emit0() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_event_emitter_listener_count() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_event_emitter_new() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_event_emitter_on() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_event_emitter_remove_all_listeners() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_event_emitter_remove_listener() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_fastify_add_hook() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_fastify_all() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_fastify_create() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_fastify_create_with_opts() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_fastify_ctx_html() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_fastify_ctx_json() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_fastify_ctx_redirect() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_fastify_ctx_text() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_fastify_delete() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_fastify_get() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_fastify_head() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_fastify_listen() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_fastify_options() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_fastify_patch() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_fastify_post() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_fastify_put() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_fastify_register() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_fastify_reply_header() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_fastify_reply_send() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_fastify_reply_status() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_fastify_req_body() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_fastify_req_header() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_fastify_req_headers() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_fastify_req_json() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_fastify_req_method() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_fastify_req_param() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_fastify_req_params() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_fastify_req_query() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_fastify_req_url() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_fastify_route() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_fastify_set_error_handler() -> i64 {
    0
}
// js_fetch_stream_* — not yet implemented for Android
#[no_mangle]
pub extern "C" fn js_fetch_stream_start() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_fetch_stream_poll() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_fetch_stream_close() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_fetch_stream_status() -> i64 {
    0
}
// js_fetch_* — real implementations in fetch.rs
#[no_mangle]
pub extern "C" fn js_get_export() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_http_request_body() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_http_request_body_length() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_http_request_content_type() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_http_request_has_header() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_http_request_header() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_http_request_headers_all() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_http_request_id() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_http_request_is_method() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_http_request_method() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_http_request_path() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_http_request_query() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_http_request_query_all() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_http_request_query_param() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_http_respond_error() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_http_respond_html() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_http_respond_json() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_http_respond_not_found() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_http_respond_redirect() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_http_respond_status_text() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_http_respond_text() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_http_respond_with_headers() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_http_server_accept_v2() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_http_server_close() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_http_server_create() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_ioredis_connect() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_ioredis_decr() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_ioredis_del() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_ioredis_disconnect() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_ioredis_exists() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_ioredis_expire() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_ioredis_get() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_ioredis_hdel() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_ioredis_hget() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_ioredis_hgetall() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_ioredis_hlen() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_ioredis_hset() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_ioredis_incr() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_ioredis_new() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_ioredis_ping() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_ioredis_quit() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_ioredis_set() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_ioredis_setex() -> i64 {
    0
}
// js_json_* — real implementations in json.rs
#[no_mangle]
pub extern "C" fn js_jwt_decode() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_jwt_sign() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_jwt_sign_es256() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_jwt_sign_rs256() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_jwt_verify() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_camel_case() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_capitalize() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_chunk() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_clamp() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_compact() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_concat() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_difference() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_drop() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_drop_right() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_ends_with() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_escape() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_first() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_flatten() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_in_range() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_includes() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_initial() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_kebab_case() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_last() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_lower_case() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_lower_first() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_pad() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_pad_end() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_pad_start() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_random() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_repeat() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_replace() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_reverse() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_size() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_snake_case() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_split() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_start_case() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_starts_with() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_tail() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_take() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_take_right() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_trim() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_trim_end() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_trim_start() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_truncate() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_unescape() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_uniq() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_upper_case() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lodash_upper_first() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lru_cache_clear() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lru_cache_delete() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lru_cache_get() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lru_cache_has() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lru_cache_new() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lru_cache_peek() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lru_cache_set() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_lru_cache_size() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_moment_add() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_moment_date() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_moment_day() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_moment_diff() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_moment_end_of() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_moment_format() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_moment_from_timestamp() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_moment_hour() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_moment_is_valid() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_moment_millisecond() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_moment_minute() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_moment_month() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_moment_now() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_moment_parse() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_moment_second() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_moment_start_of() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_moment_subtract() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_moment_unix() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_moment_value_of() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_moment_year() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_mongodb_client_close() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_mongodb_client_db() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_mongodb_collection_count() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_mongodb_collection_delete_many() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_mongodb_collection_delete_one() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_mongodb_collection_find() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_mongodb_collection_find_one() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_mongodb_collection_insert_many() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_mongodb_collection_insert_one() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_mongodb_collection_update_many() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_mongodb_collection_update_one() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_mongodb_connect() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_mongodb_db_collection() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_mysql2_connection_begin_transaction() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_mysql2_connection_commit() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_mysql2_connection_end() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_mysql2_connection_execute() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_mysql2_connection_query() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_mysql2_connection_rollback() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_mysql2_create_connection() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_mysql2_create_pool() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_mysql2_pool_connection_execute() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_mysql2_pool_connection_query() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_mysql2_pool_connection_release() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_mysql2_pool_end() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_mysql2_pool_execute() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_mysql2_pool_get_connection() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_mysql2_pool_query() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_nanoid() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_nanoid_custom() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_new_instance() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_nodemailer_create_transport() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_nodemailer_send_mail() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_nodemailer_verify() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_pg_client_end() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_pg_client_query() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_pg_client_query_params() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_pg_connect() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_pg_create_pool() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_pg_pool_end() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_pg_pool_query() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_ratelimit_block() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_ratelimit_consume() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_ratelimit_create() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_ratelimit_delete() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_ratelimit_get() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_ratelimit_penalty() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_ratelimit_reward() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_runtime_init() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_set_property() -> i64 {
    0
}
// Issue #552: js_sharp_* symbols (blur / flip / flop / from_buffer / from_file
// / grayscale / metadata / resize / rotate / to_buffer / to_file) now provided
// by perry-ext-sharp via the `extern crate` reference in lib.rs.
// js_sharp_negate / js_sharp_quality / js_sharp_to_format remain as no-op
// stubs in perry-runtime/src/closure.rs (those three FFIs were never wired in
// either perry-stdlib or perry-ext-sharp).
#[no_mangle]
pub extern "C" fn js_slugify() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_slugify_strict() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_sqlite_close() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_sqlite_exec() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_sqlite_open() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_sqlite_prepare() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_sqlite_stmt_all() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_sqlite_stmt_get() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_sqlite_stmt_run() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_sqlite_transaction() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_sqlite_transaction_commit() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_sqlite_transaction_rollback() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_uuid_nil() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_uuid_v1() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_uuid_v4() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_uuid_v7() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_uuid_validate() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_uuid_version() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_validator_contains() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_validator_equals() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_validator_is_alpha() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_validator_is_alphanumeric() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_validator_is_email() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_validator_is_empty() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_validator_is_float() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_validator_is_hexadecimal() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_validator_is_int() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_validator_is_json() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_validator_is_length() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_validator_is_lowercase() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_validator_is_numeric() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_validator_is_uppercase() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_validator_is_url() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_validator_is_uuid() -> i64 {
    0
}
// readline (#347) — TUI use case isn't relevant on Android, so stubs
// return inert values (handle 0, no-op for everything). The `_active`
// stub returns 0 so the host event loop doesn't keep ticking.
#[no_mangle]
pub extern "C" fn js_readline_create_interface() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_readline_question() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_readline_on() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_readline_close() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_readline_process_pending() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_readline_has_active() -> i64 {
    0
}
// Phase 2 stubs.
#[no_mangle]
pub extern "C" fn js_readline_set_raw_mode(_enabled: f64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_readline_stdin_on(_event_ptr: i64, _callback: i64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_readline_stdin_remove_listener(_event_ptr: i64, _callback: i64) -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_readline_stdin_pause() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_readline_stdin_resume() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_readline_stdin_unref() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_readline_stdin_ref() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_readline_stdin_destroy() -> i64 {
    0
}
// Phase 3 stubs are NOT here — js_tty_* / js_process_*_isatty /
// js_process_stdout_columns/rows/on live in perry-runtime/src/tty.rs
// which is always linked, so no Android stub is needed.
#[no_mangle]
pub extern "C" fn js_worker_threads_get_worker_data() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_worker_threads_has_pending() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_worker_threads_on() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_worker_threads_parent_port() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_worker_threads_post_message() -> i64 {
    0
}
// js_ws_* — now provided by perry-runtime/stdlib_stubs.rs
#[no_mangle]
pub extern "C" fn js_zlib_deflate_sync() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_zlib_gunzip() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_zlib_gunzip_sync() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_zlib_gzip() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_zlib_gzip_sync() -> i64 {
    0
}
#[no_mangle]
pub extern "C" fn js_zlib_inflate_sync() -> i64 {
    0
}
