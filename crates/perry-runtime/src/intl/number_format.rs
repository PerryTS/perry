use super::*;

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

/// The ten digit glyphs for a Unicode numbering system, or `None` for `latn`
/// (and any unrecognized system, which falls back to Latin digits). Generated
/// from the test262 `numberingSystemDigits` table; covers every system with a
/// simple sequential-or-tabulated digit mapping.
pub(crate) fn numbering_system_digits(name: &str) -> Option<[char; 10]> {
    match name {
        // BEGIN generated numbering-system digit table
        "adlm" => Some([
            '\u{1e950}',
            '\u{1e951}',
            '\u{1e952}',
            '\u{1e953}',
            '\u{1e954}',
            '\u{1e955}',
            '\u{1e956}',
            '\u{1e957}',
            '\u{1e958}',
            '\u{1e959}',
        ]),
        "ahom" => Some([
            '\u{11730}',
            '\u{11731}',
            '\u{11732}',
            '\u{11733}',
            '\u{11734}',
            '\u{11735}',
            '\u{11736}',
            '\u{11737}',
            '\u{11738}',
            '\u{11739}',
        ]),
        "arab" => Some([
            '\u{660}', '\u{661}', '\u{662}', '\u{663}', '\u{664}', '\u{665}', '\u{666}', '\u{667}',
            '\u{668}', '\u{669}',
        ]),
        "arabext" => Some([
            '\u{6f0}', '\u{6f1}', '\u{6f2}', '\u{6f3}', '\u{6f4}', '\u{6f5}', '\u{6f6}', '\u{6f7}',
            '\u{6f8}', '\u{6f9}',
        ]),
        "bali" => Some([
            '\u{1b50}', '\u{1b51}', '\u{1b52}', '\u{1b53}', '\u{1b54}', '\u{1b55}', '\u{1b56}',
            '\u{1b57}', '\u{1b58}', '\u{1b59}',
        ]),
        "beng" => Some([
            '\u{9e6}', '\u{9e7}', '\u{9e8}', '\u{9e9}', '\u{9ea}', '\u{9eb}', '\u{9ec}', '\u{9ed}',
            '\u{9ee}', '\u{9ef}',
        ]),
        "bhks" => Some([
            '\u{11c50}',
            '\u{11c51}',
            '\u{11c52}',
            '\u{11c53}',
            '\u{11c54}',
            '\u{11c55}',
            '\u{11c56}',
            '\u{11c57}',
            '\u{11c58}',
            '\u{11c59}',
        ]),
        "brah" => Some([
            '\u{11066}',
            '\u{11067}',
            '\u{11068}',
            '\u{11069}',
            '\u{1106a}',
            '\u{1106b}',
            '\u{1106c}',
            '\u{1106d}',
            '\u{1106e}',
            '\u{1106f}',
        ]),
        "cakm" => Some([
            '\u{11136}',
            '\u{11137}',
            '\u{11138}',
            '\u{11139}',
            '\u{1113a}',
            '\u{1113b}',
            '\u{1113c}',
            '\u{1113d}',
            '\u{1113e}',
            '\u{1113f}',
        ]),
        "cham" => Some([
            '\u{aa50}', '\u{aa51}', '\u{aa52}', '\u{aa53}', '\u{aa54}', '\u{aa55}', '\u{aa56}',
            '\u{aa57}', '\u{aa58}', '\u{aa59}',
        ]),
        "deva" => Some([
            '\u{966}', '\u{967}', '\u{968}', '\u{969}', '\u{96a}', '\u{96b}', '\u{96c}', '\u{96d}',
            '\u{96e}', '\u{96f}',
        ]),
        "diak" => Some([
            '\u{11950}',
            '\u{11951}',
            '\u{11952}',
            '\u{11953}',
            '\u{11954}',
            '\u{11955}',
            '\u{11956}',
            '\u{11957}',
            '\u{11958}',
            '\u{11959}',
        ]),
        "fullwide" => Some([
            '\u{ff10}', '\u{ff11}', '\u{ff12}', '\u{ff13}', '\u{ff14}', '\u{ff15}', '\u{ff16}',
            '\u{ff17}', '\u{ff18}', '\u{ff19}',
        ]),
        "gara" => Some([
            '\u{10d40}',
            '\u{10d41}',
            '\u{10d42}',
            '\u{10d43}',
            '\u{10d44}',
            '\u{10d45}',
            '\u{10d46}',
            '\u{10d47}',
            '\u{10d48}',
            '\u{10d49}',
        ]),
        "gong" => Some([
            '\u{11da0}',
            '\u{11da1}',
            '\u{11da2}',
            '\u{11da3}',
            '\u{11da4}',
            '\u{11da5}',
            '\u{11da6}',
            '\u{11da7}',
            '\u{11da8}',
            '\u{11da9}',
        ]),
        "gonm" => Some([
            '\u{11d50}',
            '\u{11d51}',
            '\u{11d52}',
            '\u{11d53}',
            '\u{11d54}',
            '\u{11d55}',
            '\u{11d56}',
            '\u{11d57}',
            '\u{11d58}',
            '\u{11d59}',
        ]),
        "gujr" => Some([
            '\u{ae6}', '\u{ae7}', '\u{ae8}', '\u{ae9}', '\u{aea}', '\u{aeb}', '\u{aec}', '\u{aed}',
            '\u{aee}', '\u{aef}',
        ]),
        "gukh" => Some([
            '\u{16130}',
            '\u{16131}',
            '\u{16132}',
            '\u{16133}',
            '\u{16134}',
            '\u{16135}',
            '\u{16136}',
            '\u{16137}',
            '\u{16138}',
            '\u{16139}',
        ]),
        "guru" => Some([
            '\u{a66}', '\u{a67}', '\u{a68}', '\u{a69}', '\u{a6a}', '\u{a6b}', '\u{a6c}', '\u{a6d}',
            '\u{a6e}', '\u{a6f}',
        ]),
        "hanidec" => Some([
            '\u{3007}', '\u{4e00}', '\u{4e8c}', '\u{4e09}', '\u{56db}', '\u{4e94}', '\u{516d}',
            '\u{4e03}', '\u{516b}', '\u{4e5d}',
        ]),
        "hmng" => Some([
            '\u{16b50}',
            '\u{16b51}',
            '\u{16b52}',
            '\u{16b53}',
            '\u{16b54}',
            '\u{16b55}',
            '\u{16b56}',
            '\u{16b57}',
            '\u{16b58}',
            '\u{16b59}',
        ]),
        "hmnp" => Some([
            '\u{1e140}',
            '\u{1e141}',
            '\u{1e142}',
            '\u{1e143}',
            '\u{1e144}',
            '\u{1e145}',
            '\u{1e146}',
            '\u{1e147}',
            '\u{1e148}',
            '\u{1e149}',
        ]),
        "java" => Some([
            '\u{a9d0}', '\u{a9d1}', '\u{a9d2}', '\u{a9d3}', '\u{a9d4}', '\u{a9d5}', '\u{a9d6}',
            '\u{a9d7}', '\u{a9d8}', '\u{a9d9}',
        ]),
        "kali" => Some([
            '\u{a900}', '\u{a901}', '\u{a902}', '\u{a903}', '\u{a904}', '\u{a905}', '\u{a906}',
            '\u{a907}', '\u{a908}', '\u{a909}',
        ]),
        "kawi" => Some([
            '\u{11f50}',
            '\u{11f51}',
            '\u{11f52}',
            '\u{11f53}',
            '\u{11f54}',
            '\u{11f55}',
            '\u{11f56}',
            '\u{11f57}',
            '\u{11f58}',
            '\u{11f59}',
        ]),
        "khmr" => Some([
            '\u{17e0}', '\u{17e1}', '\u{17e2}', '\u{17e3}', '\u{17e4}', '\u{17e5}', '\u{17e6}',
            '\u{17e7}', '\u{17e8}', '\u{17e9}',
        ]),
        "knda" => Some([
            '\u{ce6}', '\u{ce7}', '\u{ce8}', '\u{ce9}', '\u{cea}', '\u{ceb}', '\u{cec}', '\u{ced}',
            '\u{cee}', '\u{cef}',
        ]),
        "krai" => Some([
            '\u{16d70}',
            '\u{16d71}',
            '\u{16d72}',
            '\u{16d73}',
            '\u{16d74}',
            '\u{16d75}',
            '\u{16d76}',
            '\u{16d77}',
            '\u{16d78}',
            '\u{16d79}',
        ]),
        "lana" => Some([
            '\u{1a80}', '\u{1a81}', '\u{1a82}', '\u{1a83}', '\u{1a84}', '\u{1a85}', '\u{1a86}',
            '\u{1a87}', '\u{1a88}', '\u{1a89}',
        ]),
        "lanatham" => Some([
            '\u{1a90}', '\u{1a91}', '\u{1a92}', '\u{1a93}', '\u{1a94}', '\u{1a95}', '\u{1a96}',
            '\u{1a97}', '\u{1a98}', '\u{1a99}',
        ]),
        "laoo" => Some([
            '\u{ed0}', '\u{ed1}', '\u{ed2}', '\u{ed3}', '\u{ed4}', '\u{ed5}', '\u{ed6}', '\u{ed7}',
            '\u{ed8}', '\u{ed9}',
        ]),
        "lepc" => Some([
            '\u{1c40}', '\u{1c41}', '\u{1c42}', '\u{1c43}', '\u{1c44}', '\u{1c45}', '\u{1c46}',
            '\u{1c47}', '\u{1c48}', '\u{1c49}',
        ]),
        "limb" => Some([
            '\u{1946}', '\u{1947}', '\u{1948}', '\u{1949}', '\u{194a}', '\u{194b}', '\u{194c}',
            '\u{194d}', '\u{194e}', '\u{194f}',
        ]),
        "mathbold" => Some([
            '\u{1d7ce}',
            '\u{1d7cf}',
            '\u{1d7d0}',
            '\u{1d7d1}',
            '\u{1d7d2}',
            '\u{1d7d3}',
            '\u{1d7d4}',
            '\u{1d7d5}',
            '\u{1d7d6}',
            '\u{1d7d7}',
        ]),
        "mathdbl" => Some([
            '\u{1d7d8}',
            '\u{1d7d9}',
            '\u{1d7da}',
            '\u{1d7db}',
            '\u{1d7dc}',
            '\u{1d7dd}',
            '\u{1d7de}',
            '\u{1d7df}',
            '\u{1d7e0}',
            '\u{1d7e1}',
        ]),
        "mathmono" => Some([
            '\u{1d7f6}',
            '\u{1d7f7}',
            '\u{1d7f8}',
            '\u{1d7f9}',
            '\u{1d7fa}',
            '\u{1d7fb}',
            '\u{1d7fc}',
            '\u{1d7fd}',
            '\u{1d7fe}',
            '\u{1d7ff}',
        ]),
        "mathsanb" => Some([
            '\u{1d7ec}',
            '\u{1d7ed}',
            '\u{1d7ee}',
            '\u{1d7ef}',
            '\u{1d7f0}',
            '\u{1d7f1}',
            '\u{1d7f2}',
            '\u{1d7f3}',
            '\u{1d7f4}',
            '\u{1d7f5}',
        ]),
        "mathsans" => Some([
            '\u{1d7e2}',
            '\u{1d7e3}',
            '\u{1d7e4}',
            '\u{1d7e5}',
            '\u{1d7e6}',
            '\u{1d7e7}',
            '\u{1d7e8}',
            '\u{1d7e9}',
            '\u{1d7ea}',
            '\u{1d7eb}',
        ]),
        "mlym" => Some([
            '\u{d66}', '\u{d67}', '\u{d68}', '\u{d69}', '\u{d6a}', '\u{d6b}', '\u{d6c}', '\u{d6d}',
            '\u{d6e}', '\u{d6f}',
        ]),
        "modi" => Some([
            '\u{11650}',
            '\u{11651}',
            '\u{11652}',
            '\u{11653}',
            '\u{11654}',
            '\u{11655}',
            '\u{11656}',
            '\u{11657}',
            '\u{11658}',
            '\u{11659}',
        ]),
        "mong" => Some([
            '\u{1810}', '\u{1811}', '\u{1812}', '\u{1813}', '\u{1814}', '\u{1815}', '\u{1816}',
            '\u{1817}', '\u{1818}', '\u{1819}',
        ]),
        "mroo" => Some([
            '\u{16a60}',
            '\u{16a61}',
            '\u{16a62}',
            '\u{16a63}',
            '\u{16a64}',
            '\u{16a65}',
            '\u{16a66}',
            '\u{16a67}',
            '\u{16a68}',
            '\u{16a69}',
        ]),
        "mtei" => Some([
            '\u{abf0}', '\u{abf1}', '\u{abf2}', '\u{abf3}', '\u{abf4}', '\u{abf5}', '\u{abf6}',
            '\u{abf7}', '\u{abf8}', '\u{abf9}',
        ]),
        "mymr" => Some([
            '\u{1040}', '\u{1041}', '\u{1042}', '\u{1043}', '\u{1044}', '\u{1045}', '\u{1046}',
            '\u{1047}', '\u{1048}', '\u{1049}',
        ]),
        "mymrepka" => Some([
            '\u{116da}',
            '\u{116db}',
            '\u{116dc}',
            '\u{116dd}',
            '\u{116de}',
            '\u{116df}',
            '\u{116e0}',
            '\u{116e1}',
            '\u{116e2}',
            '\u{116e3}',
        ]),
        "mymrpao" => Some([
            '\u{116d0}',
            '\u{116d1}',
            '\u{116d2}',
            '\u{116d3}',
            '\u{116d4}',
            '\u{116d5}',
            '\u{116d6}',
            '\u{116d7}',
            '\u{116d8}',
            '\u{116d9}',
        ]),
        "mymrshan" => Some([
            '\u{1090}', '\u{1091}', '\u{1092}', '\u{1093}', '\u{1094}', '\u{1095}', '\u{1096}',
            '\u{1097}', '\u{1098}', '\u{1099}',
        ]),
        "mymrtlng" => Some([
            '\u{a9f0}', '\u{a9f1}', '\u{a9f2}', '\u{a9f3}', '\u{a9f4}', '\u{a9f5}', '\u{a9f6}',
            '\u{a9f7}', '\u{a9f8}', '\u{a9f9}',
        ]),
        "nagm" => Some([
            '\u{1e4f0}',
            '\u{1e4f1}',
            '\u{1e4f2}',
            '\u{1e4f3}',
            '\u{1e4f4}',
            '\u{1e4f5}',
            '\u{1e4f6}',
            '\u{1e4f7}',
            '\u{1e4f8}',
            '\u{1e4f9}',
        ]),
        "newa" => Some([
            '\u{11450}',
            '\u{11451}',
            '\u{11452}',
            '\u{11453}',
            '\u{11454}',
            '\u{11455}',
            '\u{11456}',
            '\u{11457}',
            '\u{11458}',
            '\u{11459}',
        ]),
        "nkoo" => Some([
            '\u{7c0}', '\u{7c1}', '\u{7c2}', '\u{7c3}', '\u{7c4}', '\u{7c5}', '\u{7c6}', '\u{7c7}',
            '\u{7c8}', '\u{7c9}',
        ]),
        "olck" => Some([
            '\u{1c50}', '\u{1c51}', '\u{1c52}', '\u{1c53}', '\u{1c54}', '\u{1c55}', '\u{1c56}',
            '\u{1c57}', '\u{1c58}', '\u{1c59}',
        ]),
        "onao" => Some([
            '\u{1e5f1}',
            '\u{1e5f2}',
            '\u{1e5f3}',
            '\u{1e5f4}',
            '\u{1e5f5}',
            '\u{1e5f6}',
            '\u{1e5f7}',
            '\u{1e5f8}',
            '\u{1e5f9}',
            '\u{1e5fa}',
        ]),
        "orya" => Some([
            '\u{b66}', '\u{b67}', '\u{b68}', '\u{b69}', '\u{b6a}', '\u{b6b}', '\u{b6c}', '\u{b6d}',
            '\u{b6e}', '\u{b6f}',
        ]),
        "osma" => Some([
            '\u{104a0}',
            '\u{104a1}',
            '\u{104a2}',
            '\u{104a3}',
            '\u{104a4}',
            '\u{104a5}',
            '\u{104a6}',
            '\u{104a7}',
            '\u{104a8}',
            '\u{104a9}',
        ]),
        "outlined" => Some([
            '\u{1ccf0}',
            '\u{1ccf1}',
            '\u{1ccf2}',
            '\u{1ccf3}',
            '\u{1ccf4}',
            '\u{1ccf5}',
            '\u{1ccf6}',
            '\u{1ccf7}',
            '\u{1ccf8}',
            '\u{1ccf9}',
        ]),
        "rohg" => Some([
            '\u{10d30}',
            '\u{10d31}',
            '\u{10d32}',
            '\u{10d33}',
            '\u{10d34}',
            '\u{10d35}',
            '\u{10d36}',
            '\u{10d37}',
            '\u{10d38}',
            '\u{10d39}',
        ]),
        "saur" => Some([
            '\u{a8d0}', '\u{a8d1}', '\u{a8d2}', '\u{a8d3}', '\u{a8d4}', '\u{a8d5}', '\u{a8d6}',
            '\u{a8d7}', '\u{a8d8}', '\u{a8d9}',
        ]),
        "segment" => Some([
            '\u{1fbf0}',
            '\u{1fbf1}',
            '\u{1fbf2}',
            '\u{1fbf3}',
            '\u{1fbf4}',
            '\u{1fbf5}',
            '\u{1fbf6}',
            '\u{1fbf7}',
            '\u{1fbf8}',
            '\u{1fbf9}',
        ]),
        "shrd" => Some([
            '\u{111d0}',
            '\u{111d1}',
            '\u{111d2}',
            '\u{111d3}',
            '\u{111d4}',
            '\u{111d5}',
            '\u{111d6}',
            '\u{111d7}',
            '\u{111d8}',
            '\u{111d9}',
        ]),
        "sind" => Some([
            '\u{112f0}',
            '\u{112f1}',
            '\u{112f2}',
            '\u{112f3}',
            '\u{112f4}',
            '\u{112f5}',
            '\u{112f6}',
            '\u{112f7}',
            '\u{112f8}',
            '\u{112f9}',
        ]),
        "sinh" => Some([
            '\u{de6}', '\u{de7}', '\u{de8}', '\u{de9}', '\u{dea}', '\u{deb}', '\u{dec}', '\u{ded}',
            '\u{dee}', '\u{def}',
        ]),
        "sora" => Some([
            '\u{110f0}',
            '\u{110f1}',
            '\u{110f2}',
            '\u{110f3}',
            '\u{110f4}',
            '\u{110f5}',
            '\u{110f6}',
            '\u{110f7}',
            '\u{110f8}',
            '\u{110f9}',
        ]),
        "sund" => Some([
            '\u{1bb0}', '\u{1bb1}', '\u{1bb2}', '\u{1bb3}', '\u{1bb4}', '\u{1bb5}', '\u{1bb6}',
            '\u{1bb7}', '\u{1bb8}', '\u{1bb9}',
        ]),
        "sunu" => Some([
            '\u{11bf0}',
            '\u{11bf1}',
            '\u{11bf2}',
            '\u{11bf3}',
            '\u{11bf4}',
            '\u{11bf5}',
            '\u{11bf6}',
            '\u{11bf7}',
            '\u{11bf8}',
            '\u{11bf9}',
        ]),
        "takr" => Some([
            '\u{116c0}',
            '\u{116c1}',
            '\u{116c2}',
            '\u{116c3}',
            '\u{116c4}',
            '\u{116c5}',
            '\u{116c6}',
            '\u{116c7}',
            '\u{116c8}',
            '\u{116c9}',
        ]),
        "talu" => Some([
            '\u{19d0}', '\u{19d1}', '\u{19d2}', '\u{19d3}', '\u{19d4}', '\u{19d5}', '\u{19d6}',
            '\u{19d7}', '\u{19d8}', '\u{19d9}',
        ]),
        "tamldec" => Some([
            '\u{be6}', '\u{be7}', '\u{be8}', '\u{be9}', '\u{bea}', '\u{beb}', '\u{bec}', '\u{bed}',
            '\u{bee}', '\u{bef}',
        ]),
        "telu" => Some([
            '\u{c66}', '\u{c67}', '\u{c68}', '\u{c69}', '\u{c6a}', '\u{c6b}', '\u{c6c}', '\u{c6d}',
            '\u{c6e}', '\u{c6f}',
        ]),
        "thai" => Some([
            '\u{e50}', '\u{e51}', '\u{e52}', '\u{e53}', '\u{e54}', '\u{e55}', '\u{e56}', '\u{e57}',
            '\u{e58}', '\u{e59}',
        ]),
        "tibt" => Some([
            '\u{f20}', '\u{f21}', '\u{f22}', '\u{f23}', '\u{f24}', '\u{f25}', '\u{f26}', '\u{f27}',
            '\u{f28}', '\u{f29}',
        ]),
        "tirh" => Some([
            '\u{114d0}',
            '\u{114d1}',
            '\u{114d2}',
            '\u{114d3}',
            '\u{114d4}',
            '\u{114d5}',
            '\u{114d6}',
            '\u{114d7}',
            '\u{114d8}',
            '\u{114d9}',
        ]),
        "tnsa" => Some([
            '\u{16ac0}',
            '\u{16ac1}',
            '\u{16ac2}',
            '\u{16ac3}',
            '\u{16ac4}',
            '\u{16ac5}',
            '\u{16ac6}',
            '\u{16ac7}',
            '\u{16ac8}',
            '\u{16ac9}',
        ]),
        "tols" => Some([
            '\u{11de0}',
            '\u{11de1}',
            '\u{11de2}',
            '\u{11de3}',
            '\u{11de4}',
            '\u{11de5}',
            '\u{11de6}',
            '\u{11de7}',
            '\u{11de8}',
            '\u{11de9}',
        ]),
        "vaii" => Some([
            '\u{a620}', '\u{a621}', '\u{a622}', '\u{a623}', '\u{a624}', '\u{a625}', '\u{a626}',
            '\u{a627}', '\u{a628}', '\u{a629}',
        ]),
        "wara" => Some([
            '\u{118e0}',
            '\u{118e1}',
            '\u{118e2}',
            '\u{118e3}',
            '\u{118e4}',
            '\u{118e5}',
            '\u{118e6}',
            '\u{118e7}',
            '\u{118e8}',
            '\u{118e9}',
        ]),
        "wcho" => Some([
            '\u{1e2f0}',
            '\u{1e2f1}',
            '\u{1e2f2}',
            '\u{1e2f3}',
            '\u{1e2f4}',
            '\u{1e2f5}',
            '\u{1e2f6}',
            '\u{1e2f7}',
            '\u{1e2f8}',
            '\u{1e2f9}',
        ]),
        // END generated numbering-system digit table
        _ => None,
    }
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
        push_sign(&mut parts, &r.sign_display, false, true);
        parts.push(("nan", "NaN".to_string()));
        push_style_suffix(&mut parts, r, decimal_sep);
        return parts;
    }

    let mut abs = value.abs();
    if r.style == "percent" {
        abs *= 100.0;
    }

    if abs.is_infinite() {
        let mut out: Vec<(&'static str, String)> = Vec::new();
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

/// Append the trailing style suffix (`percent`/`unit`) after the numeric parts.
pub(crate) fn push_style_suffix(
    parts: &mut Vec<(&'static str, String)>,
    r: &NfResolved,
    _decimal_sep: char,
) {
    match r.style.as_str() {
        "percent" => parts.push(("percentSign", "%".to_string())),
        "unit" => {
            if let Some(unit) = &r.unit {
                parts.push(("literal", " ".to_string()));
                parts.push(("unit", unit.clone()));
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
    // The native float renderer below doesn't honor roundingIncrement; when set,
    // snap the magnitude onto the increment grid first (digit-string rounding,
    // respecting roundingMode) so the renderer formats an already-gridded value.
    let value = if r.rounding_increment != 1.0 && value.is_finite() {
        let negative = value < 0.0 || (value == 0.0 && value.is_sign_negative());
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
    let digits = format_number_parts(value, locale, Some(frac_digits), None);
    let mut numeric: Vec<(&'static str, String)> = Vec::new();
    split_numeric_parts(&digits, locale, &mut numeric);
    let mut parts: Vec<(&'static str, String)> = Vec::new();
    match r.currency.as_deref() {
        Some("EUR") if locale.starts_with("de") => {
            parts = numeric;
            parts.push(("literal", "\u{00a0}".to_string()));
            parts.push(("currency", "\u{20ac}".to_string()));
        }
        Some("EUR") => {
            parts.push(("currency", "\u{20ac}".to_string()));
            parts.extend(numeric);
        }
        Some("USD") => {
            parts.push(("currency", "$".to_string()));
            parts.extend(numeric);
        }
        Some(code) => {
            parts = numeric;
            parts.push(("literal", " ".to_string()));
            parts.push(("currency", code.to_string()));
        }
        None => parts = numeric,
    }
    parts
}

pub(crate) fn format_number_instance(obj: *const ObjectHeader, value: f64) -> String {
    number_instance_parts(obj, value)
        .iter()
        .map(|(_, v)| v.as_str())
        .collect()
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
