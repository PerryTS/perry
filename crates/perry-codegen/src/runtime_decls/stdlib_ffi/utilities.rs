//! Utility-package stdlib FFI declarations (extracted from stdlib_ffi.rs):
//! @perryts/pdf, decimal.js, ethers, lodash.

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
}
