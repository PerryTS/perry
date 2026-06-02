//! Number-to-string formatting helpers (`Number.prototype.toString`,
//! `.toFixed`, `.toPrecision`, `.toExponential`).

use super::*;

/// Cached small-integer string table (0..=255). Initialized lazily on
/// first access. Avoids gc_malloc + format! for commonly repeated
/// number-to-string conversions (loop counters, property name suffixes).
///
/// Thread-local: each thread (perry/thread workers and the main thread)
/// has its own arena, so cached pointers MUST be per-thread — sharing
/// across threads would hand back arena pointers that are invalid in
/// the caller's address space (use-after-free / cross-arena UB).
const SMALL_INT_CACHE_SIZE: usize = 256;
thread_local! {
    static SMALL_INT_CACHE: std::cell::UnsafeCell<[*mut StringHeader; SMALL_INT_CACHE_SIZE]> =
        const { std::cell::UnsafeCell::new([std::ptr::null_mut(); SMALL_INT_CACHE_SIZE]) };
}

/// Convert a number (f64) to a string
/// Returns a new string representing the number
#[no_mangle]
pub extern "C" fn js_number_to_string(value: f64) -> *mut StringHeader {
    // Fast path: small non-negative integers use a cached string table.
    if value.fract() == 0.0 && value >= 0.0 && value < SMALL_INT_CACHE_SIZE as f64 {
        let idx = value as usize;
        let cached = SMALL_INT_CACHE.with(|c| unsafe { (*c.get())[idx] });
        if !cached.is_null() {
            return cached;
        }
        // Allocate and cache
        let s = format!("{}", value as u64);
        let ptr = js_string_from_bytes_longlived(s.as_bytes().as_ptr(), s.len() as u32);
        unsafe {
            // Mark as shared so it's never mutated in-place
            (*ptr).refcount = 0;
            // Mark as pinned so GC keeps it live for the lifetime of this
            // thread's arena.
            let gc_header =
                (ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
            (*gc_header).gc_flags |= crate::gc::GC_FLAG_PINNED;
        }
        SMALL_INT_CACHE.with(|c| unsafe {
            // GC_STORE_AUDIT(ROOT): SMALL_INT_CACHE is scanned by scan_small_int_cache_roots_mut.
            crate::gc::runtime_store_root_raw_mut_ptr_slot(&raw mut (*c.get())[idx], ptr);
        });
        return ptr;
    }

    // Format the number as a string per JS semantics.
    let s = js_format_f64(value);

    let bytes = s.as_bytes();
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

/// ECMAScript `Number::toString` formatting, returning the Rust `String`.
///
/// Shared by `js_number_to_string` (the `.toString()` path) and the
/// string-concat fast paths so `String(n)`, `"" + n`, and `` `${n}` `` all
/// match Node — notably scientific notation when `|n| >= 1e21` or
/// `|n| < 1e-6` (#3987). Previously the concat fast paths used a bare
/// `format!("{}", n)`, which emits the full decimal form (e.g.
/// `1000000000000000000000` for `1e21`) and could even truncate
/// `Number.MAX_VALUE`'s ~309-digit decimal into a fixed stack buffer.
pub(crate) fn js_format_f64(value: f64) -> String {
    if value.is_nan() {
        "NaN".to_string()
    } else if value.is_infinite() {
        if value > 0.0 {
            "Infinity".to_string()
        } else {
            "-Infinity".to_string()
        }
    } else if value == 0.0 {
        // Cover both +0 and -0 as "0" (matches JS)
        "0".to_string()
    } else if value.fract() == 0.0 && value.abs() < 1e15 {
        // Integer-like, format without decimal
        format!("{}", value as i64)
    } else {
        // ECMAScript NumberToString: switch to scientific notation when
        // |n| >= 10^21 or |n| < 10^-6 (otherwise Rust's `{}` produces
        // 300-digit decimals for `Number.MAX_VALUE` and 16-digit
        // 0.000…0002… decimals for `Number.EPSILON`, neither of which
        // matches Node's output).
        let abs = value.abs();
        if !(1e-6..1e21).contains(&abs) {
            fix_exponent_format(&format!("{:e}", value))
        } else {
            format!("{}", value)
        }
    }
}

/// GC root scanner for the small-integer string cache.
///
/// The cache stores raw `StringHeader*` values, not NaN-boxed JSValues. The
/// entries are allocated long-lived and pinned before publication, and this
/// scanner keeps the slots visible to moving-GC verification/rewrite paths.
pub fn scan_small_int_cache_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_small_int_cache_roots_mut(&mut visitor);
}

pub fn scan_small_int_cache_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    SMALL_INT_CACHE.with(|c| unsafe {
        for slot in (*c.get()).iter_mut() {
            let mut addr = *slot as usize;
            if visitor.visit_tagged_usize_slot(&mut addr, crate::value::STRING_TAG) {
                *slot = addr as *mut StringHeader;
            }
        }
    });
}

fn is_undefined_arg(value: f64) -> bool {
    value.to_bits() == crate::value::TAG_UNDEFINED
}

fn to_integer_or_infinity(value: f64) -> f64 {
    let number = crate::builtins::js_number_coerce(value);
    if number.is_nan() || number == 0.0 {
        0.0
    } else if number.is_infinite() {
        number
    } else {
        number.trunc()
    }
}

fn throw_number_format_range_error(message: &str) -> ! {
    let msg = js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_rangeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

#[cfg(test)]
pub(crate) fn test_seed_small_int_cache_root(index: usize, string_ptr: usize) {
    let idx = index % SMALL_INT_CACHE_SIZE;
    SMALL_INT_CACHE.with(|c| unsafe {
        // GC_STORE_AUDIT(ROOT): test seed mirrors SMALL_INT_CACHE roots scanned by scan_small_int_cache_roots_mut.
        crate::gc::runtime_store_root_raw_mut_ptr_slot(
            &raw mut (*c.get())[idx],
            string_ptr as *mut StringHeader,
        );
    });
}

#[cfg(test)]
pub(crate) fn test_small_int_cache_root(index: usize) -> usize {
    let idx = index % SMALL_INT_CACHE_SIZE;
    SMALL_INT_CACHE.with(|c| unsafe { (*c.get())[idx] as usize })
}

#[cfg(test)]
pub(crate) fn test_clear_small_int_cache_root(index: usize) {
    let idx = index % SMALL_INT_CACHE_SIZE;
    SMALL_INT_CACHE.with(|c| unsafe {
        // GC_STORE_AUDIT(ROOT): test clear writes a non-pointer sentinel into scanned SMALL_INT_CACHE roots.
        crate::gc::runtime_store_root_raw_mut_ptr_slot(
            &raw mut (*c.get())[idx],
            std::ptr::null_mut(),
        );
    });
}

/// Format a number with a fixed number of decimal places (Number.prototype.toFixed).
///
/// Hot path on CSV/log/template-build workloads (`(i * 1.5).toFixed(2)`
/// in a 100k-iteration loop showed 21 ms in this fn alone vs Bun's 6 ms
/// — 3.5× slower, dominated by Rust's general f64 → decimal formatter
/// inside `format!`).
///
/// **Integer-arithmetic fast path** (`fmt_fixed_int`): for the common
/// case (`dp ≤ 6`, `|value| < 1e15`), multiply by `10^dp`, round to the
/// nearest i64, then write integer-part + "." + zero-padded fractional-
/// part directly into a stack 64-byte buffer. No heap allocation, no
/// general formatter machinery — pure integer arithmetic + digit
/// emission. This is the same algorithm V8 / SpiderMonkey use for the
/// fast path of toFixed.
///
/// Falls back to `format!` for NaN/Infinity, large values that need
/// general scientific-notation handling, or precision > 6 where i64
/// overflow becomes a real risk.
#[no_mangle]
pub extern "C" fn js_number_to_fixed(value: f64, decimals: f64) -> *mut StringHeader {
    let dp_number = to_integer_or_infinity(decimals);
    if !(0.0..=100.0).contains(&dp_number) {
        throw_number_format_range_error("toFixed() digits argument must be between 0 and 100");
    }
    let dp = dp_number as usize;

    if value.is_nan() || value.is_infinite() {
        return js_number_to_string(value);
    }

    // ECMA-262 §21.1.3.3 step 9: if |x| >= 10^21, the result is ToString(x)
    // (which switches to exponential form), NOT a zero-padded fixed string.
    // `format!("{:.prec$}", 1e21)` would emit "1000000000000000000000.00";
    // Node emits "1e+21".
    if value.abs() >= 1e21 {
        return js_number_to_string(value);
    }

    // Fast path: pure integer arithmetic + manual digit emission.
    // Conditions: finite, magnitude < 1e15 (so value * 10^dp fits safely
    // in i64), dp <= 6 (limits 10^dp to 1_000_000 — `value * 10^dp` then
    // stays under 1e21, well inside i64's ~9.2e18 range).
    if value.abs() < 1e15 && dp <= 6 {
        if let Some(n) = fmt_fixed_int(value, dp) {
            return n;
        }
    }

    // Slow path: Rust formatter handles NaN/Infinity, very large values,
    // and high-precision cases.
    let s = format!("{:.prec$}", value, prec = dp);
    let bytes = s.as_bytes();
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

/// Hand-rolled `toFixed` formatter for the common case. Returns None if
/// the value falls outside the fast-path's safe range; the caller falls
/// back to `format!` in that case.
#[inline]
fn fmt_fixed_int(value: f64, dp: usize) -> Option<*mut StringHeader> {
    // Powers of 10 up to 10^6 — kept small so the multiplication stays
    // inside i64 even for `|value|` near 1e15.
    static POW10: [u64; 7] = [1, 10, 100, 1_000, 10_000, 100_000, 1_000_000];
    let scale = POW10[dp];

    // The multiplication `value * scale` can land on a half-integer in
    // two very different ways, which `toFixed` must round oppositely:
    //
    //   * Genuine half (e.g. `2.5`, `1234.5`, `0.5`): the exact real
    //     product `value * scale` IS k + 0.5. ECMA-262 §21.1.3.3 picks
    //     the larger n on a tie — i.e. round half away from zero — so
    //     `(2.5).toFixed(0)` is "3", not "2".
    //   * Precision artifact (e.g. `0.015 * 100`): the f64 product
    //     rounds to exactly 1.5, but the true value is 1.499999… (the
    //     IEEE-754 value of 0.015 is 0.01499999…). Here Node's Grisu
    //     formatter rounds the *true* value down to "0.01".
    //
    // Rust's `f64::round` is round-half-away-from-zero, so the genuine
    // case is handled by `scaled_raw.round()` below. Only the artifact
    // case must defer to `format!` (Grisu, operating on the true value).
    //
    // Distinguish them by testing whether the multiply was *exact*: an
    // FMA computes `value*scale - scaled_raw` at infinite precision, so
    // a zero error means `scaled_raw` is the exact product and a 0.5
    // fractional part is a genuine tie. A non-zero error means the half
    // is a rounding artifact — let `format!` decide on the true value.
    let s = scale as f64;
    let scaled_raw = value * s;
    let frac = scaled_raw - scaled_raw.floor();
    // 1e-9 catches any plausible f64-precision artifact: the relative
    // error of one f64 mul on values < 1e15 is bounded by ~1e-15, and
    // we're working with values whose fractional part is in [0, 1).
    if (frac - 0.5).abs() < 1e-9 {
        let err = value.mul_add(s, -scaled_raw);
        if err != 0.0 {
            // Inexact product → artifact half. Defer to Grisu.
            return None;
        }
        // Exact product → genuine half. Fall through; `round()` rounds
        // away from zero, matching V8 / the spec's larger-n tiebreak.
    }
    let scaled = scaled_raw.round();
    if !scaled.is_finite() {
        return None;
    }

    // Extract sign + magnitude as i64. We've already gated value.abs() <
    // 1e15 + dp ≤ 6, so `scaled` is at most ~1e21 — outside i64 range.
    // Re-check after rounding: i64 max is ~9.22e18, so `scaled.abs() < 1e18`
    // is the actual safe bound. Bail to slow path if we overshoot.
    if scaled.abs() >= 9_000_000_000_000_000_000.0 {
        return None;
    }
    let neg = scaled < 0.0;
    let abs_n = scaled.abs() as u64;

    // Buffer big enough for: '-' + up to 19 integer digits + '.' + 6
    // fractional digits + 1 slack = 27 bytes. 32 is plenty.
    let mut buf = [0u8; 32];
    let mut len = 0;

    let int_part = abs_n / scale;
    let frac_part = abs_n % scale;

    if neg {
        buf[len] = b'-';
        len += 1;
    }

    // Write integer part (at least one digit, even when 0).
    if int_part == 0 {
        buf[len] = b'0';
        len += 1;
    } else {
        // Build digits in reverse, then copy into buf in forward order.
        let mut tmp = [0u8; 20];
        let mut tmp_len = 0;
        let mut n = int_part;
        while n > 0 {
            tmp[tmp_len] = b'0' + (n % 10) as u8;
            tmp_len += 1;
            n /= 10;
        }
        for i in 0..tmp_len {
            buf[len + i] = tmp[tmp_len - 1 - i];
        }
        len += tmp_len;
    }

    // Fractional part: only if dp > 0. Zero-pad to exactly `dp` digits.
    if dp > 0 {
        buf[len] = b'.';
        len += 1;
        // Build dp-digit fractional in reverse with zero-padding.
        let mut frac = frac_part;
        for i in (0..dp).rev() {
            buf[len + i] = b'0' + (frac % 10) as u8;
            frac /= 10;
        }
        len += dp;
    }

    Some(js_string_from_ascii_bytes(buf.as_ptr(), len as u32))
}

/// Format a number with a precision (Number.prototype.toPrecision).
/// JS spec: total significant digits, switches to exponential for very small/large.
#[no_mangle]
pub extern "C" fn js_number_to_precision(value: f64, precision: f64) -> *mut StringHeader {
    let s = if is_undefined_arg(precision) {
        format_number_for_js(value)
    } else if value.is_nan() {
        "NaN".to_string()
    } else if value.is_infinite() {
        if value > 0.0 {
            "Infinity".to_string()
        } else {
            "-Infinity".to_string()
        }
    } else {
        let p_number = to_integer_or_infinity(precision);
        if p_number < 1.0 || p_number > 100.0 {
            throw_number_format_range_error("toPrecision() argument must be between 1 and 100");
        }
        let p = p_number as usize;
        if value == 0.0 {
            // 0.toPrecision(3) = "0.00"
            if p == 1 {
                "0".to_string()
            } else {
                format!("0.{}", "0".repeat(p - 1))
            }
        } else {
            // Find the decimal exponent: floor(log10(|x|))
            let abs = value.abs();
            let exp = abs.log10().floor() as i32;
            // JS uses exponential notation when exp < -6 or exp >= precision
            if exp < -6 || exp >= p as i32 {
                // Exponential: precision-1 digits after decimal, e+/-exp
                let mantissa_digits = p.saturating_sub(1);
                let formatted = format!("{:.*e}", mantissa_digits, value);
                // Rust's "{:e}" format produces "1.23e4"; JS uses "1.23e+4"
                fix_exponent_format(&formatted)
            } else {
                // Fixed: precision - exp - 1 digits after decimal
                let dp = (p as i32 - exp - 1).max(0) as usize;
                format!("{:.prec$}", value, prec = dp)
            }
        }
    };
    let bytes = s.as_bytes();
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

/// Format a number in exponential notation (Number.prototype.toExponential).
#[no_mangle]
pub extern "C" fn js_number_to_exponential(value: f64, decimals: f64) -> *mut StringHeader {
    let s = if value.is_nan() {
        "NaN".to_string()
    } else if value.is_infinite() {
        if value > 0.0 {
            "Infinity".to_string()
        } else {
            "-Infinity".to_string()
        }
    } else if is_undefined_arg(decimals) {
        fix_exponent_format(&format!("{:e}", value))
    } else {
        let dp_number = to_integer_or_infinity(decimals);
        if !(0.0..=100.0).contains(&dp_number) {
            throw_number_format_range_error("toExponential() argument must be between 0 and 100");
        }
        let dp = dp_number as usize;
        // Rust's `{:e}` produces e.g. "1.23e4"; JS expects "1.23e+4"
        let formatted = format!("{:.*e}", dp, value);
        fix_exponent_format(&formatted)
    };
    let bytes = s.as_bytes();
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

/// Convert Rust's `{:e}` exponential format to JS's: "1.23e4" -> "1.23e+4", "1.23e-4" stays.
pub(crate) fn fix_exponent_format(s: &str) -> String {
    if let Some(e_pos) = s.find('e') {
        let (mantissa, exp_part) = s.split_at(e_pos);
        let exp_str = &exp_part[1..]; // skip 'e'
        if exp_str.starts_with('-') {
            format!("{}e{}", mantissa, exp_str)
        } else {
            // Add explicit + sign and strip leading zeros from exponent
            let n: i64 = exp_str.parse().unwrap_or(0);
            format!("{}e+{}", mantissa, n)
        }
    } else {
        s.to_string()
    }
}

/// Format a number per JS toString rules (helper for toPrecision when precision=0)
fn format_number_for_js(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 {
            "Infinity".to_string()
        } else {
            "-Infinity".to_string()
        };
    }
    if value == 0.0 {
        return "0".to_string();
    }
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        // ECMAScript NumberToString — see js_number_to_string for rationale.
        let abs = value.abs();
        if !(1e-6..1e21).contains(&abs) {
            fix_exponent_format(&format!("{:e}", value))
        } else {
            format!("{}", value)
        }
    }
}
