//! Utility-package stdlib FFI declarations (extracted from stdlib_ffi.rs):
//! @perryts/pdf, commander, dotenv, date libs (dayjs/datefns/moment),
//! decimal.js, ethers, lodash, lru-cache.

use crate::module::LlModule;
use crate::types::{DOUBLE, I64, VOID};

pub(crate) fn declare_utilities(module: &mut LlModule) {
    // ========== @perryts/pdf (issue #516) ==========
    // createPdf returns an i64 handle (NaN-boxed POINTER_TAG by
    // codegen via NR_PTR). The mutator ops are Rust `-> ()` and
    // therefore VOID at the LLVM ABI level.
    module.declare_function("js_pdf_create_pdf", I64, &[DOUBLE]);
    module.declare_function("js_pdf_add_text", VOID, &[I64, I64, DOUBLE, DOUBLE, DOUBLE]);
    module.declare_function(
        "js_pdf_add_line",
        VOID,
        &[I64, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
    );
    module.declare_function("js_pdf_new_page", VOID, &[I64]);
    module.declare_function("js_pdf_save", VOID, &[I64]);

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
    module.declare_function(
        "js_datefns_difference_in_minutes",
        DOUBLE,
        &[DOUBLE, DOUBLE],
    );
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
    module.declare_function("js_dayjs_factory", DOUBLE, &[I64]);
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
    // moment: same handle scheme as dayjs — the factory returns the
    // handle as f64 bits (DOUBLE), instance methods take the handle as
    // an I64 first arg. Methods returning a new moment return DOUBLE
    // (f64::from_bits(handle)). Keep in lock-step with the moment rows
    // in lower_call/native_table/dates.rs and the runtime signatures in
    // perry-stdlib/src/moment.rs + perry-ext-moment.
    module.declare_function("js_moment_add", DOUBLE, &[I64, DOUBLE, I64]);
    module.declare_function("js_moment_clone", DOUBLE, &[I64]);
    module.declare_function("js_moment_date", DOUBLE, &[I64]);
    module.declare_function("js_moment_day", DOUBLE, &[I64]);
    module.declare_function("js_moment_diff", DOUBLE, &[I64, I64, I64]);
    module.declare_function("js_moment_end_of", DOUBLE, &[I64, I64]);
    module.declare_function("js_moment_factory", DOUBLE, &[I64]);
    module.declare_function("js_moment_format", I64, &[I64, I64]);
    module.declare_function("js_moment_from_now", I64, &[I64]);
    module.declare_function("js_moment_from_timestamp", DOUBLE, &[DOUBLE]);
    module.declare_function("js_moment_hour", DOUBLE, &[I64]);
    module.declare_function("js_moment_is_after", DOUBLE, &[I64, I64]);
    module.declare_function("js_moment_is_before", DOUBLE, &[I64, I64]);
    module.declare_function("js_moment_is_between", DOUBLE, &[I64, I64, I64]);
    module.declare_function("js_moment_is_same", DOUBLE, &[I64, I64, I64]);
    module.declare_function("js_moment_is_valid", DOUBLE, &[I64]);
    module.declare_function("js_moment_millisecond", DOUBLE, &[I64]);
    module.declare_function("js_moment_minute", DOUBLE, &[I64]);
    module.declare_function("js_moment_month", DOUBLE, &[I64]);
    module.declare_function("js_moment_now", DOUBLE, &[]);
    module.declare_function("js_moment_parse", DOUBLE, &[I64]);
    module.declare_function("js_moment_second", DOUBLE, &[I64]);
    module.declare_function("js_moment_start_of", DOUBLE, &[I64, I64]);
    module.declare_function("js_moment_subtract", DOUBLE, &[I64, DOUBLE, I64]);
    module.declare_function("js_moment_to_date", DOUBLE, &[I64]);
    module.declare_function("js_moment_to_iso_string", I64, &[I64]);
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
    module.declare_function("js_lodash_max", DOUBLE, &[I64]);
    module.declare_function("js_lodash_max_by", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_lodash_mean", DOUBLE, &[I64]);
    module.declare_function("js_lodash_mean_by", DOUBLE, &[I64, DOUBLE]);
    module.declare_function("js_lodash_min", DOUBLE, &[I64]);
    module.declare_function("js_lodash_min_by", DOUBLE, &[I64, DOUBLE]);
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
    module.declare_function("js_lodash_sum", DOUBLE, &[I64]);
    module.declare_function("js_lodash_sum_by", DOUBLE, &[I64, DOUBLE]);
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
}
