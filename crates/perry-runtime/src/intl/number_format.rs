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
    let (mode, neg) = ROUND_CTX.with(|c| c.get());
    let first = dropped.first().copied().unwrap_or(b'0');
    let rest_zero = dropped[1..].iter().all(|&d| d == b'0');
    let exactly_half = first == b'5' && rest_zero;
    let more_half = first > b'5' || (first == b'5' && !rest_zero);
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
        ROUND_HALF_EVEN => more_half || (exactly_half && (last_kept - b'0') % 2 == 1),
        _ => half_or_more, // halfExpand (default)
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
        None => ("0".to_string(), String::new()),
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

/// Build the typed parts from an already-resolved [`NfResolved`] (the shared
/// rendering core behind `format` / `formatToParts`).
pub(crate) fn number_parts_from_resolved(
    r: &NfResolved,
    value: f64,
) -> Vec<(&'static str, String)> {
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
            let exp = if r.notation == "engineering" {
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
            let (mut i_out, f_out) = if r.use_sig {
                round_to_significant(m_int, m_frac, r.min_sig, r.max_sig)
            } else {
                round_to_fraction(m_int, m_frac, r.max_frac as usize)
            };
            // Significant rounding already normalizes trailing zeros; only the
            // fraction path trims down to the minimum fraction count.
            let f_out = if r.use_sig {
                f_out
            } else {
                trim_fraction(&f_out, r.min_frac as usize)
            };
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
                let (i, f) = round_to_fraction(int_part, frac_part, r.max_frac as usize);
                (i, trim_fraction(&f, r.min_frac as usize))
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
        let (i, f) = round_to_fraction(int_part, frac_part, r.max_frac as usize);
        (i, trim_fraction(&f, r.min_frac as usize))
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
    let digits = format_number_parts(
        value,
        locale,
        Some(r.currency.as_deref().map_or(2, currency_fraction_digits) as usize),
        None,
    );
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
