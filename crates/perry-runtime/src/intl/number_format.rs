use super::*;

use super::number_format_digits::numbering_system_digits;
use crate::array::{js_array_alloc, js_array_get_f64, js_array_length, js_array_push_f64};
use crate::closure::ClosureHeader;
use crate::object::{
    js_object_alloc, js_object_get_field_by_name_f64, js_object_set_field_by_name,
    set_builtin_property_attrs, ObjectHeader, PropertyAttrs,
};
use crate::string::{js_string_from_bytes, str_bytes_from_jsvalue};
use crate::value::{js_jsvalue_to_string, js_nanbox_pointer, JSValue};
use crate::StringHeader;
#[cfg(feature = "intl-segmenter")]
use unicode_segmentation::UnicodeSegmentation;

pub(crate) struct NfResolved {
    pub(crate) locale: String,
    pub(crate) numbering_system: String,
    pub(crate) style: String,
    pub(crate) currency: Option<String>,
    pub(crate) currency_display: String,
    pub(crate) currency_sign: String,
    pub(crate) unit: Option<String>,
    pub(crate) unit_display: String,
    pub(crate) notation: String,
    pub(crate) compact_display: String,
    pub(crate) sign_display: String,
    pub(crate) use_grouping: String,
    pub(crate) min_int: u32,
    /// Whether the formatter rounds by significant digits (also true for the
    /// default compact path, which uses 1–2 significant digits).
    pub(crate) use_sig: bool,
    /// Compact's default rounding surfaces *both* fraction and significant slots
    /// in `resolvedOptions` (rounding priority morePrecision).
    pub(crate) compact_both: bool,
    pub(crate) min_sig: u32,
    pub(crate) max_sig: u32,
    pub(crate) min_frac: u32,
    pub(crate) max_frac: u32,
    pub(crate) rounding_increment: f64,
    pub(crate) rounding_mode: String,
    pub(crate) rounding_priority: String,
    pub(crate) trailing_zero: String,
}

pub(crate) fn nf_load(obj: *const ObjectHeader) -> NfResolved {
    let num = |key: &str, default: f64| get_number_field(obj, key).unwrap_or(default);
    NfResolved {
        locale: get_string_field(obj, KEY_LOCALE).unwrap_or_else(|| "en-US".to_string()),
        numbering_system: get_string_field(obj, KEY_NF_NUMBERING)
            .unwrap_or_else(|| "latn".to_string()),
        style: get_string_field(obj, KEY_STYLE).unwrap_or_else(|| "decimal".to_string()),
        currency: get_string_field(obj, KEY_CURRENCY),
        currency_display: get_string_field(obj, KEY_NF_CURRENCY_DISPLAY)
            .unwrap_or_else(|| "symbol".to_string()),
        currency_sign: get_string_field(obj, KEY_NF_CURRENCY_SIGN)
            .unwrap_or_else(|| "standard".to_string()),
        unit: get_string_field(obj, KEY_NF_UNIT),
        unit_display: get_string_field(obj, KEY_NF_UNIT_DISPLAY)
            .unwrap_or_else(|| "short".to_string()),
        notation: get_string_field(obj, KEY_NF_NOTATION).unwrap_or_else(|| "standard".to_string()),
        compact_display: get_string_field(obj, KEY_NF_COMPACT_DISPLAY)
            .unwrap_or_else(|| "short".to_string()),
        sign_display: get_string_field(obj, KEY_NF_SIGN_DISPLAY)
            .unwrap_or_else(|| "auto".to_string()),
        use_grouping: get_string_field(obj, KEY_NF_USE_GROUPING)
            .unwrap_or_else(|| "auto".to_string()),
        min_int: num(KEY_NF_MIN_INT, 1.0) as u32,
        use_sig: matches!(
            get_string_field(obj, KEY_NF_USE_SIG).as_deref(),
            Some("significant") | Some("both")
        ),
        compact_both: get_string_field(obj, KEY_NF_USE_SIG).as_deref() == Some("both"),
        min_sig: num(KEY_NF_MIN_SIG, 1.0) as u32,
        max_sig: num(KEY_NF_MAX_SIG, 21.0) as u32,
        min_frac: num(KEY_NF_MIN_FRAC, 0.0) as u32,
        max_frac: num(KEY_MAX_FRACTION_DIGITS, 3.0) as u32,
        rounding_increment: num(KEY_NF_ROUNDING_INCREMENT, 1.0),
        rounding_mode: get_string_field(obj, KEY_NF_ROUNDING_MODE)
            .unwrap_or_else(|| "halfExpand".to_string()),
        rounding_priority: get_string_field(obj, KEY_NF_ROUNDING_PRIORITY)
            .unwrap_or_else(|| "auto".to_string()),
        trailing_zero: get_string_field(obj, KEY_NF_TRAILING_ZERO)
            .unwrap_or_else(|| "auto".to_string()),
    }
}

/// A resolved decimal `Intl.NumberFormat` with all spec defaults, for `locale`.
/// Callers tweak the few fields they need (`Intl.DurationFormat` formats each
/// unit value through this to stay byte-identical with a nested NumberFormat).
pub(crate) fn nf_resolved_default(locale: &str) -> NfResolved {
    NfResolved {
        locale: locale.to_string(),
        numbering_system: "latn".to_string(),
        style: "decimal".to_string(),
        currency: None,
        currency_display: "symbol".to_string(),
        currency_sign: "standard".to_string(),
        unit: None,
        unit_display: "short".to_string(),
        notation: "standard".to_string(),
        compact_display: "short".to_string(),
        sign_display: "auto".to_string(),
        use_grouping: "auto".to_string(),
        min_int: 1,
        use_sig: false,
        compact_both: false,
        min_sig: 1,
        max_sig: 21,
        min_frac: 0,
        max_frac: 3,
        rounding_increment: 1.0,
        rounding_mode: "halfExpand".to_string(),
        rounding_priority: "auto".to_string(),
        trailing_zero: "auto".to_string(),
    }
}

/// Increment a big-endian ASCII-digit buffer by one, prepending a leading `1`
/// on overflow (`"999"` → `"1000"`).
pub(crate) fn increment_decimal(digits: &mut Vec<u8>) {
    for d in digits.iter_mut().rev() {
        if *d == b'9' {
            *d = b'0';
        } else {
            *d += 1;
            return;
        }
    }
    digits.insert(0, b'1');
}

pub(crate) fn strip_leading_zeros(s: String) -> String {
    let trimmed = s.trim_start_matches('0');
    if trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

const ROUND_CEIL: u8 = 0;
const ROUND_FLOOR: u8 = 1;
const ROUND_EXPAND: u8 = 2;
const ROUND_TRUNC: u8 = 3;
const ROUND_HALF_CEIL: u8 = 4;
const ROUND_HALF_FLOOR: u8 = 5;
const ROUND_HALF_EXPAND: u8 = 6;
const ROUND_HALF_TRUNC: u8 = 7;
const ROUND_HALF_EVEN: u8 = 8;

thread_local! {
    /// (roundingMode code, value-is-negative) for the in-progress format. Set
    /// once per `number_instance_parts` call and consumed by the digit-string
    /// rounding helpers, avoiding threading the pair through every call site.
    static ROUND_CTX: std::cell::Cell<(u8, bool)> =
        const { std::cell::Cell::new((ROUND_HALF_EXPAND, false)) };
}

pub(crate) fn round_mode_code(mode: &str) -> u8 {
    match mode {
        "ceil" => ROUND_CEIL,
        "floor" => ROUND_FLOOR,
        "expand" => ROUND_EXPAND,
        "trunc" => ROUND_TRUNC,
        "halfCeil" => ROUND_HALF_CEIL,
        "halfFloor" => ROUND_HALF_FLOOR,
        "halfTrunc" => ROUND_HALF_TRUNC,
        "halfEven" => ROUND_HALF_EVEN,
        _ => ROUND_HALF_EXPAND,
    }
}

pub(crate) fn set_round_ctx(mode: &str, negative: bool) {
    ROUND_CTX.with(|c| c.set((round_mode_code(mode), negative)));
}

/// Decide whether to round the kept digits up given the dropped tail, the active
/// rounding mode, and the value's sign (ECMA-402 ApplyUnsignedRoundingMode +
/// signed direction). `last_kept` is the final retained digit (for halfEven).
pub(crate) fn rounding_up(last_kept: u8, dropped: &[u8]) -> bool {
    if dropped.iter().all(|&d| d == b'0') {
        return false; // exact — never rounds.
    }
    let first = dropped.first().copied().unwrap_or(b'0');
    let rest_zero = dropped[1..].iter().all(|&d| d == b'0');
    let exactly_half = first == b'5' && rest_zero;
    let more_half = first > b'5' || (first == b'5' && !rest_zero);
    round_decision(more_half, exactly_half, (last_kept - b'0') % 2 == 1)
}

/// Core ECMA-402 ApplyUnsignedRoundingMode decision, shared by fraction- and
/// increment-rounding. `more_half`/`exactly_half` classify the dropped remainder
/// against half the rounding unit; `kept_is_odd` is the parity of the retained
/// quantity (only consulted for `halfEven`). Returns whether to round the
/// magnitude up. Callers must guarantee a nonzero remainder before calling —
/// the directional modes (`ceil`/`floor`/`expand`) round up unconditionally.
fn round_decision(more_half: bool, exactly_half: bool, kept_is_odd: bool) -> bool {
    let (mode, neg) = ROUND_CTX.with(|c| c.get());
    let half_or_more = more_half || exactly_half;
    match mode {
        ROUND_CEIL => !neg,
        ROUND_FLOOR => neg,
        ROUND_EXPAND => true,
        ROUND_TRUNC => false,
        ROUND_HALF_CEIL => {
            if neg {
                more_half
            } else {
                half_or_more
            }
        }
        ROUND_HALF_FLOOR => {
            if neg {
                half_or_more
            } else {
                more_half
            }
        }
        ROUND_HALF_TRUNC => more_half,
        ROUND_HALF_EVEN => more_half || (exactly_half && kept_is_odd),
        _ => half_or_more, // halfExpand (default)
    }
}

/// Remainder of a big-endian ASCII decimal-digit string modulo a small divisor.
/// Folds digit-by-digit so the operand width is unbounded (the scaled integer in
/// increment rounding can exceed `u128` for wide `maximumFractionDigits`).
fn decimal_mod_small(digits: &[u8], divisor: u64) -> u64 {
    let mut r: u64 = 0;
    for &d in digits {
        r = (r * 10 + (d - b'0') as u64) % divisor;
    }
    r
}

/// Add a small value into a big-endian ASCII decimal-digit string in place,
/// prepending digits on overflow (`"99"` + 5 → `"104"`).
fn decimal_add_small(digits: &mut Vec<u8>, add: u64) {
    let mut carry = add;
    let mut i = digits.len();
    while carry > 0 {
        if i == 0 {
            digits.insert(0, b'0');
            i = 1;
        }
        i -= 1;
        let sum = (digits[i] - b'0') as u64 + carry;
        digits[i] = (sum % 10) as u8 + b'0';
        carry = sum / 10;
    }
}

/// Subtract a small value (assumed ≤ the represented number) from a big-endian
/// ASCII decimal-digit string in place (`"104"` − 5 → `"099"`).
fn decimal_sub_small(digits: &mut [u8], sub: u64) {
    let mut borrow = sub;
    let mut i = digits.len();
    while borrow > 0 {
        i -= 1;
        let s = (borrow % 10) as i64;
        borrow /= 10;
        let mut v = (digits[i] - b'0') as i64 - s;
        if v < 0 {
            v += 10;
            borrow += 1;
        }
        digits[i] = v as u8 + b'0';
    }
}

/// Round the decimal `int_part.frac_part` to the nearest multiple of
/// `increment` at exactly `frac_digits` fractional places, under the active
/// rounding mode (ECMA-402 `roundingIncrement`). Returns `(int, frac)` with the
/// fraction zero-padded to `frac_digits`. `roundingIncrement` is only resolvable
/// alongside fixed fraction-digit rounding (`minimumFractionDigits ==
/// maximumFractionDigits`, no significant digits), so a single fraction width
/// fully describes the result.
pub(crate) fn round_to_increment(
    int_part: &str,
    frac_part: &str,
    frac_digits: usize,
    increment: u128,
) -> (String, String) {
    // Scale by 10^frac_digits so the increment acts on integers: the first
    // `int_len + frac_digits` digits form the scaled integer `q`; any remaining
    // digits are the dropped fractional tail used to break ties. `q` is kept as a
    // digit string (it can exceed u128 for wide fraction scales / large values),
    // and reduced via small-divisor modular arithmetic; the sanctioned increment
    // (≤ 5000) and the dropped tail (bounded by the shortest decimal) stay small.
    let mut combined: Vec<u8> = Vec::with_capacity(int_part.len() + frac_part.len());
    combined.extend(int_part.bytes());
    combined.extend(frac_part.bytes());
    let cut = int_part.len() + frac_digits;
    while combined.len() < cut {
        combined.push(b'0');
    }
    let dropped = combined[cut..].to_vec();
    let mut q_digits = combined[..cut].to_vec();
    let inc = increment as u64;
    let dropped_zero = dropped.iter().all(|&d| d == b'0');
    // The dropped tail comes from the shortest round-trip decimal, so it fits
    // u128; an unexpectedly long tail falls back to plain fraction rounding.
    let dropped_int: u128 = if dropped.is_empty() {
        0
    } else {
        match std::str::from_utf8(&dropped).unwrap().parse() {
            Ok(v) => v,
            Err(_) => return round_to_fraction(int_part, frac_part, frac_digits),
        }
    };
    let rem = decimal_mod_small(&q_digits, inc);
    if !(rem == 0 && dropped_zero) {
        // Position of `rem.dropped` within [0, increment): compare against
        // increment/2 by cross-multiplying out the dropped fraction
        // (2·rem·10^k + 2·dropped) vs increment·10^k, with k = dropped digits.
        let classify = || -> Option<(bool, bool)> {
            let pow10 = 10u128.checked_pow(dropped.len() as u32)?;
            let lhs = (rem as u128)
                .checked_mul(2)?
                .checked_mul(pow10)?
                .checked_add(dropped_int.checked_mul(2)?)?;
            let rhs = increment.checked_mul(pow10)?;
            Some((lhs > rhs, lhs == rhs))
        };
        let Some((more_half, exactly_half)) = classify() else {
            return round_to_fraction(int_part, frac_part, frac_digits);
        };
        // Parity of q/increment (consulted only by halfEven): q ≡ rem (mod inc),
        // so `q mod 2·inc` is `rem` for an even quotient and `rem+inc` for odd.
        let kept_is_odd = decimal_mod_small(&q_digits, inc.saturating_mul(2)) != rem;
        // Round down to the lower multiple, then up one increment if required.
        decimal_sub_small(&mut q_digits, rem);
        if round_decision(more_half, exactly_half, kept_is_odd) {
            decimal_add_small(&mut q_digits, inc);
        }
    }
    // Place the decimal point `frac_digits` from the right of the scaled integer.
    while q_digits.len() <= frac_digits {
        q_digits.insert(0, b'0');
    }
    let split = q_digits.len() - frac_digits;
    let int_str = String::from_utf8(q_digits[..split].to_vec()).unwrap();
    let frac_str = String::from_utf8(q_digits[split..].to_vec()).unwrap();
    (strip_leading_zeros(int_str), frac_str)
}

/// Fraction-rounding step shared by every notation path. Honors
/// `roundingIncrement` when set (which ECMA-402 only permits alongside fixed
/// `minFrac == maxFrac` fraction-digit rounding, so no trailing-zero trimming is
/// needed); otherwise rounds to `maxFrac` places and trims down to `minFrac`.
/// With `roundingIncrement == 1` this is byte-identical to a bare
/// `round_to_fraction` + `trim_fraction`.
pub(crate) fn round_fraction_or_increment(
    int_part: &str,
    frac_part: &str,
    r: &NfResolved,
) -> (String, String) {
    if r.rounding_increment != 1.0 {
        round_to_increment(
            int_part,
            frac_part,
            r.max_frac as usize,
            r.rounding_increment as u128,
        )
    } else {
        let (i, f) = round_to_fraction(int_part, frac_part, r.max_frac as usize);
        (i, trim_fraction(&f, r.min_frac as usize))
    }
}

/// Round the decimal value `int_part.frac_part` to exactly `frac_digits`
/// fractional places under the active rounding mode, operating on the digit
/// strings so the result is independent of the binary float's representation
/// error. Returns `(integer_digits, fraction_digits)`, fraction zero-padded.
pub(crate) fn round_to_fraction(
    int_part: &str,
    frac_part: &str,
    frac_digits: usize,
) -> (String, String) {
    let int_len = int_part.len();
    let cut = int_len + frac_digits;
    let mut combined: Vec<u8> = Vec::with_capacity(cut + 1);
    combined.extend(int_part.bytes());
    combined.extend(frac_part.bytes());
    let dropped: Vec<u8> = combined.iter().skip(cut).copied().collect();
    let mut kept: Vec<u8> = combined.iter().take(cut).copied().collect();
    while kept.len() < cut {
        kept.push(b'0');
    }
    let last_kept = kept.last().copied().unwrap_or(b'0');
    if rounding_up(last_kept, &dropped) {
        increment_decimal(&mut kept);
    }
    let new_int_len = kept.len() - frac_digits;
    let int_str = String::from_utf8(kept[..new_int_len].to_vec()).unwrap();
    let frac_str = String::from_utf8(kept[new_int_len..].to_vec()).unwrap();
    (strip_leading_zeros(int_str), frac_str)
}

/// Round an integer digit string to drop its `place` least-significant digits,
/// replacing them with zeros, under the active rounding mode. `12345`, place 3 →
/// `12000`.
pub(crate) fn round_integer_to_place(int_part: &str, place: usize) -> String {
    if place >= int_part.len() {
        // The whole value sits below the rounding unit: every digit is dropped,
        // left-padded with the implied zeros above the most-significant digit.
        let mut dropped = vec![b'0'; place - int_part.len()];
        dropped.extend(int_part.bytes());
        let mut out = if rounding_up(b'0', &dropped) {
            vec![b'1']
        } else {
            Vec::new()
        };
        out.extend(std::iter::repeat(b'0').take(place));
        return strip_leading_zeros(String::from_utf8(out).unwrap());
    }
    let keep = int_part.len() - place;
    let dropped: Vec<u8> = int_part.as_bytes()[keep..].to_vec();
    let last_kept = int_part.as_bytes()[keep - 1];
    let mut kept: Vec<u8> = int_part[..keep].bytes().collect();
    if rounding_up(last_kept, &dropped) {
        increment_decimal(&mut kept);
    }
    kept.extend(std::iter::repeat(b'0').take(place));
    strip_leading_zeros(String::from_utf8(kept).unwrap())
}

/// Count significant digits in a `(int, frac)` decimal (leading zeros excluded,
/// interior/trailing digits included).
pub(crate) fn significant_count(int_part: &str, frac_part: &str) -> usize {
    let mut combined = String::with_capacity(int_part.len() + frac_part.len());
    combined.push_str(int_part);
    combined.push_str(frac_part);
    combined.trim_start_matches('0').len()
}

/// Round to `max_sig` significant digits, then ensure at least `min_sig` by
/// padding the fraction with trailing zeros. Returns `(int, frac)`.
pub(crate) fn round_to_significant(
    int_part: &str,
    frac_part: &str,
    min_sig: u32,
    max_sig: u32,
) -> (String, String) {
    let combined: String = format!("{int_part}{frac_part}");
    let first_sig = combined.bytes().position(|d| d != b'0');
    let (mut int_out, mut frac_out) = match first_sig {
        // Zero has no significant digit to anchor on: render it as a single "0"
        // padded to `min_sig` total displayed digits (minSig 3 → "0.00"). Return
        // early — the trailing-zero normalization below assumes a nonzero value
        // and would otherwise spin forever (significant_count is always 0 here).
        None => {
            let frac = "0".repeat(min_sig.max(1).saturating_sub(1) as usize);
            return ("0".to_string(), frac);
        }
        Some(fs) => {
            let msd_exp = int_part.len() as i32 - 1 - fs as i32;
            let frac_needed = max_sig as i32 - 1 - msd_exp;
            if frac_needed >= 0 {
                round_to_fraction(int_part, frac_part, frac_needed as usize)
            } else {
                (
                    round_integer_to_place(int_part, (-frac_needed) as usize),
                    String::new(),
                )
            }
        }
    };
    // Normalize trailing fraction zeros to land within [min_sig, max_sig]
    // significant digits — rounding may have produced extras (9.999→"10.0").
    while frac_out.ends_with('0') && significant_count(&int_out, &frac_out) > min_sig as usize {
        frac_out.pop();
    }
    while significant_count(&int_out, &frac_out) < min_sig as usize {
        frac_out.push('0');
    }
    if int_out.is_empty() {
        int_out.push('0');
    }
    (int_out, frac_out)
}

/// Trim trailing fraction zeros down to `min_frac` places.
pub(crate) fn trim_fraction(frac: &str, min_frac: usize) -> String {
    let mut f = frac.to_string();
    while f.len() > min_frac && f.ends_with('0') {
        f.pop();
    }
    f
}

/// Most-significant-digit decimal exponent of `abs > 0`, derived from the
/// shortest round-trip decimal so it is exact for integers.
pub(crate) fn decimal_msd_exponent(int_part: &str, frac_part: &str) -> i32 {
    let combined: String = format!("{int_part}{frac_part}");
    match combined.bytes().position(|d| d != b'0') {
        Some(fs) => int_part.len() as i32 - 1 - fs as i32,
        None => 0,
    }
}

/// Group an integer digit string into locale parts. Pushes `integer`/`group`
/// segments. Grouping is applied when `grouping` is true and the integer has >3
/// digits.
pub(crate) fn push_grouped_integer(
    parts: &mut Vec<(&'static str, String)>,
    int_digits: &str,
    group_sep: char,
    grouping: bool,
) {
    if !grouping || int_digits.len() <= 3 {
        parts.push(("integer", int_digits.to_string()));
        return;
    }
    let chars: Vec<char> = int_digits.chars().collect();
    let n = chars.len();
    let head = if n % 3 == 0 { 3 } else { n % 3 };
    parts.push(("integer", chars[..head].iter().collect()));
    let mut i = head;
    while i < n {
        parts.push(("group", group_sep.to_string()));
        parts.push(("integer", chars[i..i + 3].iter().collect()));
        i += 3;
    }
}

/// Whether grouping separators should be emitted for an integer of `int_len`
/// digits under the resolved `useGrouping` value.
pub(crate) fn grouping_enabled(use_grouping: &str, int_len: usize) -> bool {
    match use_grouping {
        "false" => false,
        "min2" => int_len >= 5,
        // "auto" / "always" both group for the locales we render (Latin/de).
        _ => int_len > 3,
    }
}

/// Locale-specific display symbol for USD. Most locales use "$"; Korean and
/// Traditional/Simplified Chinese use "US$" to disambiguate from local dollars.
fn usd_symbol(locale: &str) -> &'static str {
    if locale.starts_with("ko") || locale.starts_with("zh") {
        "US$"
    } else {
        "$"
    }
}

/// Locale-specific NaN string (e.g. zh-TW uses "非數值").
fn nan_string(locale: &str) -> &'static str {
    if locale.starts_with("zh") {
        "非數值"
    } else {
        "NaN"
    }
}

/// Compact-notation suffix tables for `en` (short and long forms).
pub(crate) fn compact_suffix(power: u32, long: bool) -> &'static str {
    match (power, long) {
        (3, false) => "K",
        (6, false) => "M",
        (9, false) => "B",
        (12, false) => "T",
        (3, true) => "thousand",
        (6, true) => "million",
        (9, true) => "billion",
        (12, true) => "trillion",
        _ => "",
    }
}

/// Append the leading sign segment per `signDisplay`. `negative` already folds in
/// the `-0` case; `is_zero` covers both signed zeros.
pub(crate) fn push_sign(
    parts: &mut Vec<(&'static str, String)>,
    sign_display: &str,
    negative: bool,
    is_zero: bool,
) {
    let seg = match sign_display {
        "never" => None,
        "always" => Some(if negative {
            ("minusSign", "-")
        } else {
            ("plusSign", "+")
        }),
        "exceptZero" => {
            if is_zero {
                None
            } else if negative {
                Some(("minusSign", "-"))
            } else {
                Some(("plusSign", "+"))
            }
        }
        "negative" => {
            if negative && !is_zero {
                Some(("minusSign", "-"))
            } else {
                None
            }
        }
        // auto
        _ => {
            if negative {
                Some(("minusSign", "-"))
            } else {
                None
            }
        }
    };
    if let Some((ty, v)) = seg {
        parts.push((ty, v.to_string()));
    }
}

/// Build the typed `formatToParts` segment list for a NumberFormat instance.
/// `format()` is defined as the concatenation of these segments' values.
pub(crate) fn number_instance_parts(
    obj: *const ObjectHeader,
    value: f64,
) -> Vec<(&'static str, String)> {
    let r = nf_load(obj);
    number_parts_from_resolved(&r, value)
}

/// Rewrite the ASCII (`latn`) digit glyphs in the numeric segments of a typed
/// parts list into the resolved numbering system. Only digit-bearing segment
/// types are touched — separators, signs, currency/unit/compact literals keep
/// their locale glyphs. A `latn` (or unknown) system is a no-op.
pub(crate) fn transliterate_parts_digits(parts: &mut [(&'static str, String)], system: &str) {
    let Some(digits) = numbering_system_digits(system) else {
        return;
    };
    for (ty, v) in parts.iter_mut() {
        if matches!(*ty, "integer" | "fraction" | "exponentInteger") {
            *v = v
                .chars()
                .map(|c| {
                    if c.is_ascii_digit() {
                        digits[(c as u8 - b'0') as usize]
                    } else {
                        c
                    }
                })
                .collect();
        }
    }
}

/// Build the typed parts from an already-resolved [`NfResolved`] (the shared
/// rendering core behind `format` / `formatToParts`), then transliterate the
/// digit glyphs into the resolved numbering system.
pub(crate) fn number_parts_from_resolved(
    r: &NfResolved,
    value: f64,
) -> Vec<(&'static str, String)> {
    let mut parts = number_parts_core(r, value);
    transliterate_parts_digits(&mut parts, &r.numbering_system);
    parts
}

/// The Latin-digit rendering core. [`number_parts_from_resolved`] wraps this to
/// apply numbering-system transliteration.
fn number_parts_core(r: &NfResolved, value: f64) -> Vec<(&'static str, String)> {
    // Currency keeps its existing locale-specific symbol rendering.
    if r.style == "currency" {
        return currency_instance_parts(r, value);
    }

    let de_style = r.locale.eq_ignore_ascii_case("de") || r.locale.starts_with("de-");
    let group_sep = if de_style { '.' } else { ',' };
    let decimal_sep = if de_style { ',' } else { '.' };

    let mut parts: Vec<(&'static str, String)> = Vec::new();
    let is_zero = value == 0.0;
    let negative = value < 0.0 || (is_zero && value.is_sign_negative());
    set_round_ctx(&r.rounding_mode, negative);

    if value.is_nan() {
        // NaN is non-negative and non-zero for sign purposes: only `always`
        // prepends a (plus) sign — `+NaN` — every other mode shows bare `NaN`.
        push_unit_prefix(&mut parts, r);
        push_sign(&mut parts, &r.sign_display, false, true);
        parts.push(("nan", nan_string(&r.locale).to_string()));
        push_style_suffix(&mut parts, r, decimal_sep);
        return parts;
    }

    let mut abs = value.abs();
    if r.style == "percent" {
        abs *= 100.0;
    }

    if abs.is_infinite() {
        let mut out: Vec<(&'static str, String)> = Vec::new();
        push_unit_prefix(&mut out, r);
        push_sign(&mut out, &r.sign_display, negative, false);
        out.push(("infinity", "∞".to_string()));
        push_style_suffix(&mut out, r, decimal_sep);
        return out;
    }

    // Exact shortest-decimal digit strings (Rust's `Display` never uses exponent).
    let shortest = format!("{abs}");
    let (int_part, frac_part) = shortest.split_once('.').unwrap_or((&shortest, ""));

    match r.notation.as_str() {
        "scientific" | "engineering" => {
            let msd = decimal_msd_exponent(int_part, frac_part);
            let mut exp = if r.notation == "engineering" {
                (msd as f64 / 3.0).floor() as i32 * 3
            } else {
                msd
            };
            // Significant digit string, decimal point placed after `int_digits` digits.
            let combined: String = format!("{int_part}{frac_part}");
            let sig_digits = combined.trim_start_matches('0');
            let sig_digits = if sig_digits.is_empty() {
                "0"
            } else {
                sig_digits
            };
            let int_digits = (msd - exp + 1).max(1) as usize;
            let (m_int, m_frac) = if sig_digits.len() >= int_digits {
                (&sig_digits[..int_digits], &sig_digits[int_digits..])
            } else {
                (sig_digits, "")
            };
            // The fraction path rounds the mantissa to `maxFrac` places (honoring
            // roundingIncrement); significant rounding already normalizes its own
            // trailing zeros.
            let (mut i_out, mut f_out) = if r.use_sig {
                round_to_significant(m_int, m_frac, r.min_sig, r.max_sig)
            } else {
                round_fraction_or_increment(m_int, m_frac, r)
            };
            // Rounding can carry the mantissa into an extra integer digit
            // (9.9 → 10); the significand is then exactly 10^(msd+1). Recompute the
            // exponent from the grown magnitude and reshape so scientific emits
            // `1E1` rather than `10E0` (engineering keeps the digit when the new
            // magnitude still falls in the same power-of-1000 band, e.g. `100E3`).
            if i_out.len() > int_digits {
                let new_msd = msd + 1;
                exp = if r.notation == "engineering" {
                    (new_msd as f64 / 3.0).floor() as i32 * 3
                } else {
                    new_msd
                };
                let new_int_digits = (new_msd - exp + 1).max(1) as usize;
                let sig = format!("{i_out}{f_out}");
                let sig = if sig.len() < new_int_digits {
                    format!("{:0<width$}", sig, width = new_int_digits)
                } else {
                    sig
                };
                i_out = sig[..new_int_digits].to_string();
                f_out = sig[new_int_digits..].to_string();
                if !r.use_sig {
                    f_out = trim_fraction(&f_out, r.min_frac as usize);
                }
            }
            while (i_out.len() as u32) < r.min_int {
                i_out.insert(0, '0');
            }
            push_grouped_integer(&mut parts, &i_out, group_sep, false);
            if !f_out.is_empty() {
                parts.push(("decimal", decimal_sep.to_string()));
                parts.push(("fraction", f_out));
            }
            parts.push(("exponentSeparator", "E".to_string()));
            if exp < 0 {
                parts.push(("exponentMinusSign", "-".to_string()));
            }
            parts.push(("exponentInteger", exp.abs().to_string()));
        }
        "compact" => {
            let mut power = if abs >= 1e12 {
                12
            } else if abs >= 1e9 {
                9
            } else if abs >= 1e6 {
                6
            } else if abs >= 1e3 {
                3
            } else {
                0
            };
            // Rounding can push the scaled value up a tier (999_999 → 999.999 →
            // rounds to 1000 → 1M, not 1000K). Re-scale until the rounded integer
            // part stays below 1000 (or we run out of suffix tiers).
            let (mut i_out, f_out) = loop {
                let (ii, ff) = if power == 0 {
                    // No scaling below the first threshold, but the same rounding
                    // applies (default compact uses morePrecision over 1–2
                    // significant digits, so 1.5 stays "1.5", not "2").
                    compact_round(int_part, frac_part, r)
                } else {
                    let scaled = format!("{}", abs / 10f64.powi(power as i32));
                    let (si, sf) = scaled.split_once('.').unwrap_or((&scaled, ""));
                    compact_round(si, sf, r)
                };
                if ii.len() > 3 && power < 12 {
                    power += 3;
                    continue;
                }
                break (ii, ff);
            };
            while (i_out.len() as u32) < r.min_int {
                i_out.insert(0, '0');
            }
            let grouping = grouping_enabled(&r.use_grouping, i_out.len());
            push_grouped_integer(&mut parts, &i_out, group_sep, grouping);
            if !f_out.is_empty() {
                parts.push(("decimal", decimal_sep.to_string()));
                parts.push(("fraction", f_out));
            }
            if power > 0 {
                let long = r.compact_display == "long";
                if long {
                    parts.push(("literal", " ".to_string()));
                }
                parts.push(("compact", compact_suffix(power, long).to_string()));
            }
        }
        _ => {
            let (mut i_out, f_out) = if r.use_sig {
                round_to_significant(int_part, frac_part, r.min_sig, r.max_sig)
            } else {
                round_fraction_or_increment(int_part, frac_part, r)
            };
            while (i_out.len() as u32) < r.min_int {
                i_out.insert(0, '0');
            }
            let grouping = grouping_enabled(&r.use_grouping, i_out.len());
            push_grouped_integer(&mut parts, &i_out, group_sep, grouping);
            if !f_out.is_empty() {
                parts.push(("decimal", decimal_sep.to_string()));
                parts.push(("fraction", f_out));
            }
        }
    }

    // Sign is decided after rounding: `exceptZero`/`negative` suppress the sign
    // when the *rounded* magnitude is zero (e.g. -0.0001 → "0"), while
    // `auto`/`always` follow the original mathematical sign (→ "-0").
    let rounded_is_zero = parts
        .iter()
        .filter(|(t, _)| *t == "integer" || *t == "fraction")
        .all(|(_, v)| v.bytes().all(|b| b == b'0'));
    let mut out: Vec<(&'static str, String)> = Vec::with_capacity(parts.len() + 2);
    push_unit_prefix(&mut out, r);
    push_sign(&mut out, &r.sign_display, negative, rounded_is_zero);
    out.append(&mut parts);
    push_style_suffix(&mut out, r, decimal_sep);
    out
}

/// Round `(int, frac)` for compact notation. The default compact path resolves
/// *both* a fraction (max 0) and a significant (1–2) candidate and keeps the more
/// precise one (roundingPriority `morePrecision`), so e.g. 1.5 stays `1.5` while
/// 999 stays `999`. Explicit significant- or fraction-only options take the
/// corresponding single path.
pub(crate) fn compact_round(int_part: &str, frac_part: &str, r: &NfResolved) -> (String, String) {
    if r.compact_both {
        let (fi, ff) = round_to_fraction(int_part, frac_part, r.max_frac as usize);
        let ff = trim_fraction(&ff, r.min_frac as usize);
        let (si, sf) = round_to_significant(int_part, frac_part, r.min_sig, r.max_sig);
        // morePrecision: the candidate with more fraction digits wins; on a tie
        // the fraction candidate is kept (ECMA-402 ToRawFixed preference).
        if sf.len() > ff.len() {
            (si, sf)
        } else {
            (fi, ff)
        }
    } else if r.use_sig {
        round_to_significant(int_part, frac_part, r.min_sig, r.max_sig)
    } else {
        round_fraction_or_increment(int_part, frac_part, r)
    }
}

/// The BCP-47 primary language subtag (`"de-DE"` → `"de"`).
fn locale_lang(locale: &str) -> &str {
    locale.split(['-', '_']).next().unwrap_or(locale)
}

/// Prefix text some locales place *before* the number for a unit (e.g. the
/// Japanese/Korean/Chinese "speed" reading of `kilometer-per-hour`'s long
/// form: "時速 -987 キロメートル"). Only a handful of compound units have a
/// CLDR-attested prefix; everything else renders suffix-only.
fn unit_prefix_text(unit: &str, display: &str, locale: &str) -> Option<&'static str> {
    if display != "long" {
        return None;
    }
    match (unit, locale_lang(locale)) {
        ("kilometer-per-hour", "ja") => Some("時速"),
        ("kilometer-per-hour", "ko") => Some("시속"),
        ("kilometer-per-hour", "zh") => Some("每小時"),
        _ => None,
    }
}

/// Suffix text CLDR renders *after* the number for a unit, plus whether a
/// literal space separates it from the number. Falls back to the raw unit
/// identifier (space-separated) for units without a hardcoded CLDR entry —
/// a placeholder, but distinct from the bare number, and byte-consistent
/// between `format`/`formatToParts`.
fn unit_suffix_text(unit: &str, display: &str, locale: &str) -> (String, bool) {
    if unit == "percent" && display != "long" {
        // The "%" glyph is the short/narrow CLDR form; long falls through to
        // the raw-identifier placeholder below like any other untabulated unit.
        return ("%".to_string(), false);
    }
    if unit == "kilometer-per-hour" {
        let lang = locale_lang(locale);
        return match (display, lang) {
            ("short", "zh") => ("公里/小時".to_string(), true),
            ("short", "ko") => ("km/h".to_string(), false),
            ("short", _) => ("km/h".to_string(), true),
            ("narrow", "de") => ("km/h".to_string(), true),
            ("narrow", "zh") => ("公里/小時".to_string(), false),
            ("narrow", _) => ("km/h".to_string(), false),
            ("long", "en") => ("kilometers per hour".to_string(), true),
            ("long", "de") => ("Kilometer pro Stunde".to_string(), true),
            ("long", "ja") => ("キロメートル".to_string(), true),
            ("long", "ko") => ("킬로미터".to_string(), false),
            ("long", "zh") => ("公里".to_string(), true),
            _ => ("km/h".to_string(), true),
        };
    }
    (unit.to_string(), true)
}

/// Prepend a unit's CLDR prefix (see [`unit_prefix_text`]), if any, before the
/// sign/number segments.
pub(crate) fn push_unit_prefix(parts: &mut Vec<(&'static str, String)>, r: &NfResolved) {
    if r.style != "unit" {
        return;
    }
    let Some(unit) = &r.unit else { return };
    if let Some(prefix) = unit_prefix_text(unit, &r.unit_display, &r.locale) {
        parts.push(("unit", prefix.to_string()));
        parts.push(("literal", " ".to_string()));
    }
}

/// Append the trailing style suffix (`percent`/`unit`) after the numeric parts.
pub(crate) fn push_style_suffix(
    parts: &mut Vec<(&'static str, String)>,
    r: &NfResolved,
    _decimal_sep: char,
) {
    match r.style.as_str() {
        // German (and most European) locales separate the percent sign from
        // the number with a non-breaking space; en-US glues it directly on
        // (test262 intl402/BigInt/prototype/toLocaleString/de-DE.js expects
        // "8.878.000.000 %", en-US.js expects "8,878,000,000%" with no
        // space — same `push_style_suffix` call, locale-gated literal).
        "percent" => {
            let de_style = r.locale.eq_ignore_ascii_case("de") || r.locale.starts_with("de-");
            if de_style {
                parts.push(("literal", "\u{a0}".to_string()));
            }
            parts.push(("percentSign", "%".to_string()));
        }
        "unit" => {
            if let Some(unit) = &r.unit {
                let (suffix, sep) = unit_suffix_text(unit, &r.unit_display, &r.locale);
                if sep {
                    parts.push(("literal", " ".to_string()));
                }
                parts.push(("unit", suffix));
            }
        }
        _ => {}
    }
}

/// Existing locale-specific currency rendering, factored out of
/// `number_instance_parts`.
pub(crate) fn currency_instance_parts(r: &NfResolved, value: f64) -> Vec<(&'static str, String)> {
    let locale = &r.locale;
    // Use the *resolved* fraction width (which already folds in the currency's
    // default digits plus any minimum/maximumFractionDigits options) so the
    // increment grid and the displayed precision agree — e.g. 3 fraction digits
    // snap on 0.005 steps, not the currency-default 0.05.
    let frac_digits = r.max_frac as usize;
    // Capture original sign before any rounding.
    let is_negative = value < 0.0 || (value == 0.0 && value.is_sign_negative());
    // CLDR doesn't define a distinct parenthesized accounting pattern for every
    // locale — German falls back to the plain minus-sign pattern even when
    // `currencySign: "accounting"` is requested.
    let accounting = r.currency_sign == "accounting" && locale_lang(locale) != "de";
    // The native float renderer below doesn't honor roundingIncrement; when set,
    // snap the magnitude onto the increment grid first (digit-string rounding,
    // respecting roundingMode) so the renderer formats an already-gridded value.
    let value = if r.rounding_increment != 1.0 && value.is_finite() {
        let negative = is_negative;
        set_round_ctx(&r.rounding_mode, negative);
        let abs = value.abs();
        let shortest = format!("{abs}");
        let (ip, fp) = shortest.split_once('.').unwrap_or((&shortest, ""));
        let (i, f) = round_to_increment(ip, fp, frac_digits, r.rounding_increment as u128);
        let mag: f64 = if f.is_empty() {
            i.parse().unwrap_or(abs)
        } else {
            format!("{i}.{f}").parse().unwrap_or(abs)
        };
        if negative {
            -mag
        } else {
            mag
        }
    } else {
        value
    };
    // Sign is rendered separately below (via `push_sign`, same as the
    // non-currency path) so always format the bare magnitude here.
    let digits = format_number_parts(value.abs(), locale, Some(frac_digits), None);
    let rounded_is_zero = value.is_nan()
        || (value.is_finite() && digits.bytes().all(|b| !b.is_ascii_digit() || b == b'0'));
    let is_negative = !value.is_nan() && is_negative;
    let mut numeric: Vec<(&'static str, String)> = Vec::new();
    split_numeric_parts(&digits, locale, &mut numeric);
    let de_style = locale.eq_ignore_ascii_case("de") || locale.starts_with("de-");
    let mut parts: Vec<(&'static str, String)> = Vec::new();
    match r.currency.as_deref() {
        Some("EUR") if de_style => {
            parts = numeric;
            parts.push(("literal", "\u{00a0}".to_string()));
            parts.push(("currency", "\u{20ac}".to_string()));
        }
        Some("EUR") => {
            parts.push(("currency", "\u{20ac}".to_string()));
            parts.extend(numeric);
        }
        Some("USD") if de_style => {
            // de-DE places the currency symbol after the number with NBSP.
            parts = numeric;
            parts.push(("literal", "\u{00a0}".to_string()));
            parts.push(("currency", usd_symbol(locale).to_string()));
        }
        Some("USD") => {
            parts.push(("currency", usd_symbol(locale).to_string()));
            parts.extend(numeric);
        }
        Some(code) => {
            parts = numeric;
            parts.push(("literal", " ".to_string()));
            parts.push(("currency", code.to_string()));
        }
        None => parts = numeric,
    }
    // Sign is decided after rounding, same rule as the non-currency path:
    // `exceptZero`/`negative` suppress the sign when the *rounded* magnitude
    // is zero (e.g. -0.0001 → no sign), while `auto`/`always` follow the
    // original mathematical sign (-0 still counts as negative).
    let mut sign_parts: Vec<(&'static str, String)> = Vec::new();
    push_sign(
        &mut sign_parts,
        &r.sign_display,
        is_negative,
        rounded_is_zero,
    );
    if accounting && sign_parts.iter().any(|(t, _)| *t == "minusSign") {
        // Accounting negatives are wrapped in parentheses instead of a minus sign.
        parts.insert(0, ("literal", "(".to_string()));
        parts.push(("literal", ")".to_string()));
    } else {
        for (i, seg) in sign_parts.into_iter().enumerate() {
            parts.insert(i, seg);
        }
    }
    parts
}

pub(crate) fn format_number_instance(obj: *const ObjectHeader, value: f64) -> String {
    number_instance_parts(obj, value)
        .iter()
        .map(|(_, v)| v.as_str())
        .collect()
}

/// Read a `StringHeader` pointer as a Rust `&str` (the data bytes immediately
/// follow the header, same layout `str_bytes_from_jsvalue` reads for a
/// NaN-boxed string value). `js_bigint_to_string`'s output is always ASCII
/// decimal digits (with an optional leading `-`), so `from_utf8_unchecked` is
/// safe here.
unsafe fn string_header_as_str<'a>(s: *const StringHeader) -> &'a str {
    let len = (*s).byte_len as usize;
    let data = (s as *const u8).add(std::mem::size_of::<StringHeader>());
    std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
}

/// Exact-precision rendering of a `BigInt` magnitude for the "standard"
/// notation `"decimal"` style — mirrors the `_ =>` (default) arm of
/// [`number_parts_core`], but starts from the BigInt's own exact base-10
/// digit string instead of an `f64`-converted one, which can't exactly
/// represent integers past 2^53 (test262 expects `90071992547409910n` to
/// round-trip exactly). Every downstream step operates on digit strings, not
/// `f64`, so only the magnitude extraction needed to change.
fn bigint_number_parts_exact(
    r: &NfResolved,
    negative: bool,
    abs_digits: &str,
) -> Vec<(&'static str, String)> {
    let de_style = r.locale.eq_ignore_ascii_case("de") || r.locale.starts_with("de-");
    let group_sep = if de_style { '.' } else { ',' };
    let decimal_sep = if de_style { ',' } else { '.' };
    set_round_ctx(&r.rounding_mode, negative);

    let mut parts: Vec<(&'static str, String)> = Vec::new();
    // `compact_round` also has the `compact_both` roundingPriority tie-break
    // branch, unreachable here (only set for `notation == "compact"`) but
    // reusing it keeps this in lockstep with that helper regardless.
    let (mut i_out, f_out) = compact_round(abs_digits, "", r);
    while (i_out.len() as u32) < r.min_int {
        i_out.insert(0, '0');
    }
    let grouping = grouping_enabled(&r.use_grouping, i_out.len());
    push_grouped_integer(&mut parts, &i_out, group_sep, grouping);
    if !f_out.is_empty() {
        parts.push(("decimal", decimal_sep.to_string()));
        parts.push(("fraction", f_out));
    }

    let rounded_is_zero = parts
        .iter()
        .filter(|(t, _)| *t == "integer" || *t == "fraction")
        .all(|(_, v)| v.bytes().all(|b| b == b'0'));
    let mut out: Vec<(&'static str, String)> = Vec::with_capacity(parts.len() + 2);
    push_sign(&mut out, &r.sign_display, negative, rounded_is_zero);
    out.append(&mut parts);
    push_style_suffix(&mut out, r, decimal_sep);
    out
}

/// `BigInt.prototype.toLocaleString(locales?, options?)` — ECMA-402
/// sec-bigint.prototype.tolocalestring (#5845). Builds a real
/// `Intl.NumberFormat` from `locales`/`options` via the same `make_instance`
/// path `new Intl.NumberFormat(...)` uses, so validation/resolution (and the
/// exceptions they throw) match exactly. Only the "standard" notation
/// `"decimal"` style renders from the BigInt's exact digit string; other
/// styles/notations fall back to the same `f64` coercion
/// `Intl.NumberFormat.prototype.format` uses (`nf_coerce_number`), so
/// `toLocaleString` stays self-consistent with `format()` there too, even
/// where both are lossy for BigInts past 2^53.
pub(crate) fn bigint_to_locale_string(value: f64, locales: f64, options: f64) -> *mut StringHeader {
    let ptr = JSValue::from_bits(value.to_bits()).as_bigint_ptr();
    let negative = unsafe { crate::bigint::js_bigint_is_negative(ptr) } != 0;
    let digits_ptr = crate::bigint::js_bigint_to_string(ptr);
    // Copy into an owned `String` right away — `digits_ptr` is GC-managed and
    // `make_instance` below allocates, which can move/free it.
    let digits = unsafe { string_header_as_str(digits_ptr) };
    let abs_digits = digits.strip_prefix('-').unwrap_or(digits).to_string();

    let nf_obj_value = make_instance(std::ptr::null(), KIND_NUMBER, locales, options);
    let nf_obj = object_ptr_from_value(nf_obj_value).expect("make_instance returns a valid object");
    let r = nf_load(nf_obj);

    let out = if r.style == "decimal" && r.notation == "standard" {
        let mut parts = bigint_number_parts_exact(&r, negative, &abs_digits);
        transliterate_parts_digits(&mut parts, &r.numbering_system);
        parts.iter().map(|(_, v)| v.as_str()).collect()
    } else {
        format_number_instance(nf_obj, nf_coerce_number(value))
    };
    js_string_from_bytes(out.as_ptr(), out.len() as u32)
}

/// Convert a typed-parts list into a JS array of `{ type, value }` objects —
/// the `Intl.*.prototype.formatToParts` return shape.
pub(crate) fn parts_to_js_array(parts: &[(&'static str, String)]) -> f64 {
    let mut arr = js_array_alloc(parts.len() as u32);
    for (ty, val) in parts {
        let obj = js_object_alloc(0, 2);
        set_field(obj, "type", string_value(ty));
        set_field(obj, "value", string_value(val));
        arr = js_array_push_f64(arr, js_nanbox_pointer(obj as i64));
    }
    js_nanbox_pointer(arr as i64)
}

pub(crate) fn this_intl_object(method: &str, expected_kind: &str) -> *mut ObjectHeader {
    let this_value = crate::object::js_implicit_this_get();
    intl_object_from_value(this_value, method, expected_kind)
}

pub(crate) fn captured_intl_object(
    closure: *const ClosureHeader,
    method: &str,
    expected_kind: &str,
) -> *mut ObjectHeader {
    let this_value = crate::closure::js_closure_get_capture_f64(closure, 0);
    intl_object_from_value(this_value, method, expected_kind)
}

pub(crate) fn intl_object_from_value(
    value: f64,
    method: &str,
    expected_kind: &str,
) -> *mut ObjectHeader {
    let Some(obj) = object_ptr_from_value(value) else {
        throw_type_error(&format!(
            "Intl.{expected_kind}.prototype.{method} called on incompatible receiver"
        ));
    };
    let kind = get_string_field(obj, KEY_KIND);
    if kind.as_deref() != Some(expected_kind) {
        throw_type_error(&format!(
            "Intl.{expected_kind}.prototype.{method} called on incompatible receiver"
        ));
    }
    obj
}

/// `get Intl.NumberFormat.prototype.format` — the ECMA-402 accessor. Validates
/// that `this` is an initialized NumberFormat (TypeError otherwise) and returns
/// the instance's bound format function ([[BoundFormat]]). It reads the hidden
/// KEY_NF_BOUND_FORMAT slot (set at construction, name `""`, length 1) rather
/// than the public own `format` property, so user mutation/deletion of that
/// property can't change what the accessor returns.
pub(crate) extern "C" fn number_format_format_getter_thunk(_closure: *const ClosureHeader) -> f64 {
    let obj = this_intl_object("format", KIND_NUMBER);
    get_field(obj, KEY_NF_BOUND_FORMAT)
}

pub(crate) extern "C" fn number_format_bound_format_thunk(
    closure: *const ClosureHeader,
    value: f64,
) -> f64 {
    let obj = captured_intl_object(closure, "format", KIND_NUMBER);
    number_format_format_object(obj, value)
}

/// Coerce the `Intl.NumberFormat.prototype.format` / `formatToParts` argument to a
/// number. Unlike `JSValue::to_number`, this parses a String operand (`"0.001"` →
/// `0.001`) — `Intl.DurationFormat` relies on it to format the fractional seconds
/// value it passes as a decimal string. This is an `f64`-precision approximation of
/// the spec's `ToIntlMathematicalValue`, not the exact-decimal mathematical value
/// (large/high-precision operands lose precision), which is adequate for the
/// formatter's rendering path.
pub(crate) fn nf_coerce_number(value: f64) -> f64 {
    crate::builtins::js_number_coerce(value)
}

pub(crate) fn number_format_format_object(obj: *const ObjectHeader, value: f64) -> f64 {
    let number = nf_coerce_number(value);
    string_value(&format_number_instance(obj, number))
}

pub(crate) extern "C" fn number_format_resolved_options_thunk(
    _closure: *const ClosureHeader,
) -> f64 {
    let obj = this_intl_object("resolvedOptions", KIND_NUMBER);
    number_format_resolved_options_object(obj)
}

pub(crate) extern "C" fn number_format_bound_resolved_options_thunk(
    closure: *const ClosureHeader,
) -> f64 {
    let obj = captured_intl_object(closure, "resolvedOptions", KIND_NUMBER);
    number_format_resolved_options_object(obj)
}

pub(crate) extern "C" fn number_format_to_parts_thunk(
    _closure: *const ClosureHeader,
    value: f64,
) -> f64 {
    let obj = this_intl_object("formatToParts", KIND_NUMBER);
    let number = nf_coerce_number(value);
    parts_to_js_array(&number_instance_parts(obj, number))
}

pub(crate) extern "C" fn number_format_bound_to_parts_thunk(
    closure: *const ClosureHeader,
    value: f64,
) -> f64 {
    let obj = captured_intl_object(closure, "formatToParts", KIND_NUMBER);
    let number = nf_coerce_number(value);
    parts_to_js_array(&number_instance_parts(obj, number))
}

/// Coerce a `formatRange`/`formatRangeToParts` endpoint to its `f64`
/// mathematical value and run the two endpoint checks ECMA-402 places before
/// rendering: `undefined` is a TypeError (the explicit `start`/`end` undefined
/// guard), and a value that coerces to NaN is a RangeError
/// (PartitionNumberRangePattern step 1). Every other value — BigInt, numeric
/// String, ±Infinity, ±0 — coerces and is formatted. Returns the clipped pair.
fn number_range_endpoints(method: &str, start: f64, end: f64) -> (f64, f64) {
    let sj = JSValue::from_bits(start.to_bits());
    let ej = JSValue::from_bits(end.to_bits());
    if sj.is_undefined() || ej.is_undefined() {
        throw_type_error(&format!(
            "Intl.NumberFormat.prototype.{method} called with undefined start or end"
        ));
    }
    let x = nf_coerce_number(start);
    let y = nf_coerce_number(end);
    if x.is_nan() || y.is_nan() {
        throw_range_error(&format!(
            "Intl.NumberFormat.prototype.{method} called with a NaN argument"
        ));
    }
    (x, y)
}

/// `Intl.NumberFormat.prototype.formatRange` — a best-effort
/// PartitionNumberRangePattern: render both endpoints with the instance's
/// formatter, mark the result approximate (`~`) whenever the two endpoints
/// produce the same string — including mathematically equal endpoints such as
/// `formatRange(3, 3)`, which ECMA-402 still renders approximately — and
/// otherwise join the two renderings with an en dash. The exact ICU
/// field-collapsing / locale range pattern is not reproduced.
pub(crate) fn number_format_range_value(
    obj: *const ObjectHeader,
    method: &str,
    start: f64,
    end: f64,
) -> f64 {
    let (x, y) = number_range_endpoints(method, start, end);
    let r = nf_load(obj);
    let sx: String = number_parts_from_resolved(&r, x)
        .iter()
        .map(|(_, v)| v.as_str())
        .collect();
    let sy: String = number_parts_from_resolved(&r, y)
        .iter()
        .map(|(_, v)| v.as_str())
        .collect();
    if sx == sy {
        string_value(&format!("~{sx}"))
    } else {
        string_value(&format!("{sx}\u{2013}{sy}"))
    }
}

/// `Intl.NumberFormat.prototype.formatRangeToParts` — the parts shape of
/// [`number_format_range_value`]. Each segment carries a `source`
/// (`"startRange"`/`"endRange"`/`"shared"`); the approximate form (endpoints
/// that render identically, equal inputs included) prepends an
/// `approximatelySign` and tags every segment `"shared"`.
pub(crate) fn number_format_range_parts_value(
    obj: *const ObjectHeader,
    method: &str,
    start: f64,
    end: f64,
) -> f64 {
    let (x, y) = number_range_endpoints(method, start, end);
    let r = nf_load(obj);
    let tag = |parts: Vec<(&'static str, String)>, source: &'static str| {
        parts.into_iter().map(move |(t, v)| (t, v, source))
    };
    let x_parts = number_parts_from_resolved(&r, x);
    let y_parts = number_parts_from_resolved(&r, y);
    let sx: String = x_parts.iter().map(|(_, v)| v.as_str()).collect();
    let sy: String = y_parts.iter().map(|(_, v)| v.as_str()).collect();
    if sx == sy {
        let mut parts: Vec<(&'static str, String, &'static str)> =
            vec![("approximatelySign", "~".to_string(), "shared")];
        parts.extend(tag(x_parts, "shared"));
        return super::date_collator::range_parts_to_js_array(&parts);
    }
    let mut parts: Vec<(&'static str, String, &'static str)> = tag(x_parts, "startRange").collect();
    parts.push(("literal", "\u{2013}".to_string(), "shared"));
    parts.extend(tag(y_parts, "endRange"));
    super::date_collator::range_parts_to_js_array(&parts)
}

pub(crate) extern "C" fn number_format_range_thunk(
    _closure: *const ClosureHeader,
    start: f64,
    end: f64,
) -> f64 {
    let obj = this_intl_object("formatRange", KIND_NUMBER);
    number_format_range_value(obj, "formatRange", start, end)
}

pub(crate) extern "C" fn number_format_range_to_parts_thunk(
    _closure: *const ClosureHeader,
    start: f64,
    end: f64,
) -> f64 {
    let obj = this_intl_object("formatRangeToParts", KIND_NUMBER);
    number_format_range_parts_value(obj, "formatRangeToParts", start, end)
}

pub(crate) fn number_format_resolved_options_object(obj: *const ObjectHeader) -> f64 {
    let r = nf_load(obj);
    let out = js_object_alloc(0, 16);
    set_field(out, "locale", string_value(&r.locale));
    set_field(out, "numberingSystem", string_value(&r.numbering_system));
    set_field(out, "style", string_value(&r.style));
    match r.style.as_str() {
        "currency" => {
            if let Some(currency) = &r.currency {
                set_field(out, "currency", string_value(currency));
            }
            set_field(out, "currencyDisplay", string_value(&r.currency_display));
            set_field(out, "currencySign", string_value(&r.currency_sign));
        }
        "unit" => {
            if let Some(unit) = &r.unit {
                set_field(out, "unit", string_value(unit));
            }
            set_field(out, "unitDisplay", string_value(&r.unit_display));
        }
        _ => {}
    }
    set_field(out, "minimumIntegerDigits", r.min_int as f64);
    if r.compact_both {
        // Compact's default rounding (morePrecision) surfaces both slots.
        set_field(out, "minimumFractionDigits", r.min_frac as f64);
        set_field(out, "maximumFractionDigits", r.max_frac as f64);
        set_field(out, "minimumSignificantDigits", r.min_sig as f64);
        set_field(out, "maximumSignificantDigits", r.max_sig as f64);
    } else if r.use_sig {
        set_field(out, "minimumSignificantDigits", r.min_sig as f64);
        set_field(out, "maximumSignificantDigits", r.max_sig as f64);
    } else {
        set_field(out, "minimumFractionDigits", r.min_frac as f64);
        set_field(out, "maximumFractionDigits", r.max_frac as f64);
    }
    if r.use_grouping == "false" {
        set_field(out, "useGrouping", bool_value(false));
    } else {
        set_field(out, "useGrouping", string_value(&r.use_grouping));
    }
    set_field(out, "notation", string_value(&r.notation));
    if r.notation == "compact" {
        set_field(out, "compactDisplay", string_value(&r.compact_display));
    }
    set_field(out, "signDisplay", string_value(&r.sign_display));
    set_field(out, "roundingIncrement", r.rounding_increment);
    set_field(out, "roundingMode", string_value(&r.rounding_mode));
    set_field(out, "roundingPriority", string_value(&r.rounding_priority));
    set_field(out, "trailingZeroDisplay", string_value(&r.trailing_zero));
    js_nanbox_pointer(out as i64)
}
