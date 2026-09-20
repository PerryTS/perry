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
crate::perry_thread_local! {
    static SMALL_INT_CACHE: std::cell::UnsafeCell<[*mut StringHeader; SMALL_INT_CACHE_SIZE]> =
        const { std::cell::UnsafeCell::new([std::ptr::null_mut(); SMALL_INT_CACHE_SIZE]) };
}

/// Cached single-ASCII-character string table (`"\0"`..`"\x7f"`), the exact
/// analogue of [`SMALL_INT_CACHE`] one dimension over: every `s[i]`,
/// `s.charAt(i)`, `[...s]` and every runtime consumer of
/// [`js_string_char_at`](super::js_string_char_at) used to MINT a fresh
/// 32-byte heap string per character read. On the compiled claude-code TUI —
/// which measures, wraps and ANSI-scans every rendered line — that is one of
/// the largest single contributors to allocation volume, and the bytes are
/// pure garbage: a one-character ASCII string has exactly 128 possible
/// contents.
///
/// Same residency contract as `SMALL_INT_CACHE`, and for the same reasons:
/// per-thread (arena pointers are not shareable), longlived-arena (so the
/// entry never anchors a nursery block), `refcount = 0` (shared — never
/// mutated in place, which is what makes handing the SAME pointer to every
/// caller sound), pinned out of the young generation, and scanned by
/// [`scan_small_int_cache_roots_mut`] so the collector rewrites the slot if
/// the longlived object is ever relocated.
const ASCII_CHAR_CACHE_SIZE: usize = 128;
crate::perry_thread_local! {
    static ASCII_CHAR_CACHE: std::cell::UnsafeCell<[*mut StringHeader; ASCII_CHAR_CACHE_SIZE]> =
        const { std::cell::UnsafeCell::new([std::ptr::null_mut(); ASCII_CHAR_CACHE_SIZE]) };
}

/// The canonical one-character string for an ASCII byte. Allocates at most
/// once per byte value per thread; every later call is a load.
pub(crate) fn ascii_char_string(byte: u8) -> *mut StringHeader {
    debug_assert!(byte < 0x80);
    let idx = (byte & 0x7f) as usize;
    let cached = ASCII_CHAR_CACHE.with(|c| unsafe { (*c.get())[idx] });
    if !cached.is_null() {
        return cached;
    }
    let ptr = js_string_from_bytes_longlived(&byte as *const u8, 1);
    unsafe {
        (*ptr).refcount = 0;
        let gc_header =
            (ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
        crate::gc::pin_object_non_young(gc_header);
    }
    ASCII_CHAR_CACHE.with(|c| unsafe {
        // GC_STORE_AUDIT(ROOT): ASCII_CHAR_CACHE is scanned by scan_small_int_cache_roots_mut.
        crate::gc::runtime_store_root_raw_mut_ptr_slot(&raw mut (*c.get())[idx], ptr);
    });
    ptr
}

/// Normalize a `Number.prototype` format-method receiver to its underlying
/// `f64`. Codegen lowers `x.toFixed(n)` / `.toExponential(n)` / `.toPrecision(n)`
/// to a direct runtime call that passes the receiver's bits as the first `f64`
/// argument. A boxed wrapper (`new Number(Infinity).toExponential(1000)`)
/// arrives as a NaN-boxed pointer — whose raw bits are themselves a NaN — so
/// without unboxing the formatters misread it as `NaN` and emit "NaN" instead
/// of "Infinity" (test262 .../toExponential/infinity.js). Plain numbers and
/// int32-tagged values pass through unchanged.
pub(crate) fn number_method_receiver(value: f64) -> f64 {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if jv.is_int32() {
        return jv.as_int32() as f64;
    }
    if jv.is_number() {
        return value;
    }
    if let Some((class_id, payload)) = crate::builtins::boxed_primitive_payload(value) {
        // 0xFFFF_00D0 == CLASS_ID_BOXED_NUMBER (see formatting::boxed_primitives).
        if class_id == 0xFFFF_00D0 {
            let pj = crate::value::JSValue::from_bits(payload.to_bits());
            if pj.is_int32() {
                return pj.as_int32() as f64;
            }
            return payload;
        }
    }
    // ECMA-262 21.1.3: `Number.prototype` is itself a Number object whose
    // [[NumberData]] is +0. The codegen-lowered `Number.prototype.toFixed(...)` /
    // `.toExponential(...)` calls land here directly (not via the brand-checking
    // thunk), so map the prototype receiver to +0 (test262
    // toFixed/S15.7.4.5_A1.1_T01.js, toExponential/this-is-0-fractiondigits-is-0.js).
    if value.to_bits() == crate::object::builtin_prototype_value("Number").to_bits() {
        return 0.0;
    }
    value
}

/// The `fractionDigits` / `precision` argument of `toFixed` / `toExponential`
/// / `toPrecision` is run through `ToIntegerOrInfinity` → `ToNumber`, and
/// `ToNumber` of a BigInt (or Symbol) throws a `TypeError` (ECMA-262 7.1.4).
/// The codegen-inline path passes the raw argument bits straight through, so
/// detect a BigInt here and throw before the numeric coercion silently
/// reinterprets the pointer (test262 toFixed/toFixed-tonumber-throws-typeerror-bigint.js).
fn throw_if_bigint_digits(arg: f64) {
    if crate::value::JSValue::from_bits(arg.to_bits()).is_bigint() {
        crate::collection_iter::throw_type_error("Cannot convert a BigInt value to a number");
    }
}

/// Convert a number (f64) to a string
/// Returns a new string representing the number
#[no_mangle]
pub extern "C" fn js_number_to_string(value: f64) -> *mut StringHeader {
    // Fast path: small non-negative integers use a cached string table.
    //
    // The admission test is `fract() == 0.0` plus an in-range check written so
    // LLVM can prove the `as u32` cannot overflow and emit a bare
    // `cvttsd2si`. The old `value as usize` — on a value the same condition
    // had already proven to be in `0..256` — lowered to Rust's full SATURATING
    // `f64 -> u64` sequence: 14 instructions of `cmov` fixup, a quarter of
    // what a cache hit cost. `-0.0` passes (`-0.0 >= 0.0`), converts to 0 and
    // returns "0", which is the spec answer for `String(-0)`. NaN and
    // +-Infinity fail `fract() == 0.0` (`fract` is `self - self.trunc()`,
    // which is NaN for both).
    if value.fract() == 0.0 && value >= 0.0 && value < SMALL_INT_CACHE_SIZE as f64 {
        let idx = value as u32 as usize;
        // SAFETY: the range test above proves `idx < SMALL_INT_CACHE_SIZE`.
        let cached = SMALL_INT_CACHE.with(|c| unsafe { *(*c.get()).get_unchecked(idx) });
        if !cached.is_null() {
            return cached;
        }
        return small_int_cache_fill(idx);
    }

    // Format the number as a string per JS semantics, on the stack.
    let mut buf = [0u8; 32];
    let len = super::concat::format_number_into(value, &mut buf);
    js_string_from_bytes(buf.as_ptr(), len as u32)
}

/// Mint, pin and publish the canonical string for a small-int cache index.
///
/// Genuinely cold: it runs at most once per index per thread — 256 times in
/// the entire life of a thread — yet inlined it put `format!`'s formatting
/// machinery, the GC pin and the root store into [`js_number_to_string`],
/// which cost every cached conversion six pushes and a 0x48-byte frame.
/// Outlined here rather than around the whole uncached tail on purpose:
/// wrapping the stack-buffer formatting path too MEASURED +11.7 instructions
/// per conversion on the float fixture, because a miss then paid an extra
/// call and re-ran the admission test.
#[cold]
#[inline(never)]
fn small_int_cache_fill(idx: usize) -> *mut StringHeader {
    debug_assert!(idx < SMALL_INT_CACHE_SIZE);
    let s = format!("{}", idx);
    let ptr = js_string_from_bytes_longlived(s.as_bytes().as_ptr(), s.len() as u32);
    unsafe {
        // Mark as shared so it's never mutated in-place
        (*ptr).refcount = 0;
        // Mark as pinned so GC keeps it live for the lifetime of this
        // thread's arena. Longlived-space (see the allocation above), so
        // this does not arm the young-pin latch (#7645).
        let gc_header =
            (ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
        crate::gc::pin_object_non_young(gc_header);
    }
    SMALL_INT_CACHE.with(|c| unsafe {
        // GC_STORE_AUDIT(ROOT): SMALL_INT_CACHE is scanned by scan_small_int_cache_roots_mut.
        crate::gc::runtime_store_root_raw_mut_ptr_slot(&raw mut (*c.get())[idx], ptr);
    });
    ptr
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
        // Rust's Display formatter also emits a shortest round-tripping
        // decimal, but it can choose the odd final digit when the two shortest
        // candidates are equidistant. ECMA-262 Number::toString requires the
        // even candidate. `ryu-js` implements that tie-break together with
        // JavaScript's fixed/scientific notation thresholds.
        let mut buffer = ryu_js::Buffer::new();
        buffer.format_finite(value).to_owned()
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
    // The single-character table rides the same scanner rather than
    // registering a 96th root scanner: both are per-thread arrays of
    // canonical `StringHeader*` with identical residency rules, and the
    // per-collection cost of every additional registered scanner is the
    // thing the collector is trying to shed.
    ASCII_CHAR_CACHE.with(|c| unsafe {
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
    let value = number_method_receiver(value);
    throw_if_bigint_digits(decimals);
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
    // The fast path multiplies `value * 10^dp` in f64 and extracts integer
    // digits, so it is only exact while that product stays below 2^53 (every
    // integer representable). `|value| < 1e15 && dp <= 6` alone permits products
    // up to 1e21 — e.g. `(94626641270.99636).toFixed(5)` computed `9.46e15`,
    // past 2^53, and the f64 rounding of the product corrupted the last digits.
    // Gate on the actual product so those defer to the exact `spec_to_fixed`
    // slow path. Refs #6079.
    // Admission for the integer fast path.
    //
    // The old bound was `dp <= 6`, justified as an i64-overflow limit but in
    // fact set by a seven-entry `POW10`: the real exactness condition sits on
    // the next line and `(6.0).toFixed(7)`, whose scaled product is 6e7 —
    // twenty orders of magnitude inside it — was refused anyway and fell into
    // the 1100-digit `spec_to_fixed`. That made `toFixed(7)` cost 11x
    // `toFixed(6)` while node and bun are flat across dp (#10770).
    //
    // dp <= 6 keeps its EXISTING condition verbatim, so nothing already on the
    // fast path changes admission, cost or output.
    //
    // dp 7..=19 is new, so it gets a PROOF instead of that heuristic: the
    // scaled product must be exactly representable, checked with the FMA
    // residual `value * scale - fl(value * scale)`. When that is zero,
    // `scaled_raw` IS the true product, so `scaled_raw.round()` is exactly the
    // spec's `n` (ECMA-262 21.1.3.3 negates first, then rounds half up on the
    // magnitude, which is what `f64::round` does away from zero). When it is
    // not zero the value is handed to the exact `spec_to_fixed` as before, so
    // the worst case of a wrong answer is not available — only a slower one.
    // `scale as f64` is exact for every table index (10^k is exact in f64 to
    // k = 22), so the residual means what it says.
    if dp < POW10_FIXED.len() {
        let scale = POW10_FIXED[dp] as f64;
        // Verbatim the condition `dp <= 6` already used, now applied at every
        // `dp` the table covers. The only edit is reading the scale out of the
        // table instead of recomputing `10u64.pow(dp)` at run time per call.
        //
        // An earlier revision of this change additionally required the scaled
        // product to be EXACT for `dp > 6` (an FMA-residual test), on the
        // theory that a newly opened range deserves a proof rather than the
        // existing heuristic. Two measurements killed it:
        //
        //   * It is REDUNDANT. `fmt_fixed_int`'s tie guard already refuses
        //     exactly the products that could round to the wrong integer, and
        //     it is dp-independent. A targeted hunt over 10,264,676 admitted
        //     probes — 3M random bit patterns plus every `dp` in 7..=19 swept
        //     0..3 ULPs either side of a `.5` boundary — found the tie guard
        //     catching 2,170,707 of them and produced ZERO cases where the
        //     exactness test changed an answer.
        //
        //   * It rejected the entire use case. Money is not exactly
        //     representable in binary: `(12.34).toFixed(8)` has an inexact
        //     scaled product and was refused, so currency and crypto amounts
        //     — the whole reason `dp >= 7` matters — stayed on the slow path
        //     at 10,227 Ir/op while the benchmark's exactly-representable
        //     `(k*1.5).toFixed(8)` showed 649. A fast path the real input
        //     cannot reach is the defect this campaign keeps finding; it does
        //     not become acceptable when it is mine.
        //
        // So: one rule for every `dp`, and the tie guard below is what makes
        // it sound. Widening the magnitude bound IS witnessed — see the
        // `2^53` compare in `fmt_fixed_int`, whose sabotage changes digits at
        // dp 16..18.
        let admissible = value.abs() < 1e15 && value.abs() * scale < 9_007_199_254_740_992.0;
        if admissible {
            if let Some(n) = fmt_fixed_int(value, dp) {
                return n;
            }
        }
    }

    // Slow path (dp > 6 or |value| >= 1e15): spec digit generation with
    // round-half-away-from-zero. Rust's `format!("{:.N}")` rounds half-to-even,
    // so `(0.00390625).toFixed(7)` gave "0.0039062" (V8 "0.0039063") and
    // `(1000000000000000.5).toFixed(0)` gave "…000" (V8 "…001"). Refs #6079.
    let s = spec_to_fixed(value, dp);
    let bytes = s.as_bytes();
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

/// Powers of ten for the `toFixed` integer fast path, and the definition of
/// how far that path reaches.
///
/// 10^19 is the largest power of ten a `u64` holds (`u64::MAX` is about
/// 1.845e19), and every entry is also exact as an `f64` (a double holds 10^k
/// exactly to k = 22). Both properties are load-bearing: `fmt_fixed_int`
/// divides by the `u64`, and `js_number_to_fixed`'s admission multiplies by
/// the `f64` and then asks whether that product was exact — a question that
/// only means anything while the scale itself is exact.
///
/// THE TABLE'S LENGTH IS THE `dp` BOUND. It used to hold seven entries while
/// the bound was spelled `dp <= 6` a hundred lines away and justified as an
/// i64-overflow limit, which is how `toFixed(7)` came to cost 11x
/// `toFixed(6)` (#10770). Anything that changes how far the fast path reaches
/// belongs here, not there.
static POW10_FIXED: [u64; 20] = [
    1,
    10,
    100,
    1_000,
    10_000,
    100_000,
    1_000_000,
    10_000_000,
    100_000_000,
    1_000_000_000,
    10_000_000_000,
    100_000_000_000,
    1_000_000_000_000,
    10_000_000_000_000,
    100_000_000_000_000,
    1_000_000_000_000_000,
    10_000_000_000_000_000,
    100_000_000_000_000_000,
    1_000_000_000_000_000_000,
    10_000_000_000_000_000_000,
];

/// Hand-rolled `toFixed` formatter for the common case. Returns None if
/// the value falls outside the fast-path's safe range; the caller falls
/// back to `format!` in that case.
#[inline]
fn fmt_fixed_int(value: f64, dp: usize) -> Option<*mut StringHeader> {
    let scale = POW10_FIXED[dp];

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
    // 2^53, not 9e18. This is what bounds the 32-byte `buf` below, now that
    // `dp` reaches 19 rather than 6: `abs_n < 2^53` is at most 16 digits, so
    // `int_part` is at most `max(1, 16 - dp)` digits and the longest possible
    // write is sign + 1 + '.' + 19 = 22 bytes. Tightening the existing compare
    // rather than adding a length check keeps the bound free - computing the
    // digit count with `ilog10` here MEASURED +12 Ir/call at dp = 2 and
    // +37 at dp = 0. Nothing is newly refused: both arms of the caller's
    // admission already require the product to be under 2^53.
    //  rather than :  is checked directly
    // above, so NaN is already excluded and the two forms agree (clippy
    // neg_cmp_op_on_partial_ord).
    if scaled.abs() >= 9_007_199_254_740_992.0 {
        return None;
    }
    // ECMA-262 §21.1.3.3 step 6 applies the sign from the ORIGINAL `x < 0`, not
    // the rounded result: a negative that rounds to zero magnitude keeps its
    // minus (`(-0.0001).toFixed(3)` → "-0.000"). Using `scaled` here dropped it,
    // because `round(-0.1)` is `-0.0` and `-0.0 < 0.0` is false. Strict `< 0.0`
    // still excludes an actual `-0` input (`(-0).toFixed(2)` → "0.00"). Refs #6079.
    let neg = value < 0.0;
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
    let value = number_method_receiver(value);
    throw_if_bigint_digits(precision);
    let s = if is_undefined_arg(precision) {
        format_number_for_js(value)
    } else {
        // ECMA-262 §21.1.3.5: `p = ? ToIntegerOrInfinity(precision)` (step 3)
        // runs *before* the non-finite check on x (step 4). A Symbol/abrupt
        // precision must therefore throw even when x is NaN/±Infinity — e.g.
        // `Number.prototype.toPrecision(Symbol())`, whose [[NumberData]] is +0
        // but which V8 also exercises with non-finite receivers (test262
        // built-ins/Number/prototype/toPrecision/return-abrupt-*-symbol).
        let p_number = to_integer_or_infinity(precision);
        if value.is_nan() {
            "NaN".to_string()
        } else if value.is_infinite() {
            if value > 0.0 {
                "Infinity".to_string()
            } else {
                "-Infinity".to_string()
            }
        } else if !(1.0..=100.0).contains(&p_number) {
            throw_number_format_range_error("toPrecision() argument must be between 1 and 100");
        } else {
            let p = p_number as usize;
            if value == 0.0 {
                // 0.toPrecision(3) = "0.00"
                if p == 1 {
                    "0".to_string()
                } else {
                    format!("0.{}", "0".repeat(p - 1))
                }
            } else {
                // ECMA-262 §21.1.3.5 selects the decimal exponent `e` from the
                // value ROUNDED to `p` significant digits (round half away from
                // zero), not the raw value. This matters at power-of-ten
                // boundaries in two ways: `log10(1e-6)` floors to -7 (float
                // error) so `(1e-6).toPrecision(1)` gave "1e-6" not "0.000001";
                // and a value that rounds UP across a power of ten (9.99e-7 →
                // 1e-6) must use the higher exponent. `spec_to_exponential`
                // performs the spec rounding and emits `[-]d.ddde±N`, so its `N`
                // IS the rounded exponent — parse it and decide fixed vs.
                // exponential from that. Also fixes `(25).toPrecision(1)` → "3e+1"
                // (Rust's `{:.*e}` rounds half-to-even). Refs #6079.
                let sci = spec_to_exponential(value, p.saturating_sub(1));
                let exp: i32 = sci
                    .rfind('e')
                    .and_then(|i| sci[i + 1..].parse().ok())
                    .unwrap_or(0);
                if exp < -6 || exp >= p as i32 {
                    sci
                } else {
                    // Fixed: precision - exp - 1 digits after decimal, spec
                    // half-away-from-zero rounding (`(1.25).toPrecision(2)` = "1.3").
                    let dp = (p as i32 - exp - 1).max(0) as usize;
                    spec_to_fixed(value, dp)
                }
            }
        }
    };
    let bytes = s.as_bytes();
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

/// Format a number in exponential notation (Number.prototype.toExponential).
#[no_mangle]
pub extern "C" fn js_number_to_exponential(value: f64, decimals: f64) -> *mut StringHeader {
    let value = number_method_receiver(value);
    throw_if_bigint_digits(decimals);
    let s = if is_undefined_arg(decimals) {
        if value.is_nan() {
            "NaN".to_string()
        } else if value.is_infinite() {
            if value > 0.0 {
                "Infinity".to_string()
            } else {
                "-Infinity".to_string()
            }
        } else {
            fix_exponent_format(&format!("{:e}", value))
        }
    } else {
        // ECMA-262 §21.1.3.2: `f = ? ToIntegerOrInfinity(fractionDigits)`
        // (step 2) runs *before* the non-finite check on x (step 3), so a
        // Symbol/abrupt fractionDigits throws even when x is NaN/±Infinity
        // (test262 .../toExponential/return-abrupt-tointeger-*-symbol).
        let dp_number = to_integer_or_infinity(decimals);
        if value.is_nan() {
            "NaN".to_string()
        } else if value.is_infinite() {
            if value > 0.0 {
                "Infinity".to_string()
            } else {
                "-Infinity".to_string()
            }
        } else if !(0.0..=100.0).contains(&dp_number) {
            throw_number_format_range_error("toExponential() argument must be between 0 and 100");
        } else {
            // ECMA-262 §21.1.3.2 selects `f+1` significant digits and, on an
            // exact tie, picks the *larger* mantissa (round half away from
            // zero). Rust's `{:.*e}` rounds half-to-even, so e.g.
            // `(25).toExponential(0)` gave "2e+1" instead of "3e+1" and
            // `(12345).toExponential(3)` gave "1.234e+4" instead of "1.235e+4".
            // Generate digits with spec rounding instead.
            spec_to_exponential(value, dp_number as usize)
        }
    };
    let bytes = s.as_bytes();
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

/// `Number.prototype.toExponential` digit generation with ECMA-262 rounding
/// (round half away from zero on a tie). `value` is finite; `dp` is the
/// fraction-digit count (already range-checked to `0..=100`).
fn spec_to_exponential(value: f64, dp: usize) -> String {
    if value == 0.0 {
        let mantissa = if dp == 0 {
            "0".to_string()
        } else {
            format!("0.{}", "0".repeat(dp))
        };
        return format!("{mantissa}e+0");
    }
    let neg = value < 0.0;
    let x = value.abs();
    // Format with far more digits than any f64 needs (≤767 significant decimal
    // digits) so the expansion is exact; then round it ourselves.
    let full = format!("{x:.1100e}");
    let epos = full.find('e').unwrap();
    let mut exp: i32 = full[epos + 1..].parse().unwrap_or(0);
    let mut digits: Vec<u8> = full[..epos]
        .bytes()
        .filter(u8::is_ascii_digit)
        .map(|b| b - b'0')
        .collect();
    let keep = dp + 1;
    if digits.len() > keep {
        // Round half away from zero: the first dropped digit decides.
        let round_up = digits[keep] >= 5;
        digits.truncate(keep);
        if round_up {
            let mut i = keep;
            loop {
                if i == 0 {
                    // Carry past the most significant digit: 9.99→10.0, i.e.
                    // mantissa becomes 1 followed by zeros and the exponent grows.
                    digits.insert(0, 1);
                    digits.truncate(keep);
                    exp += 1;
                    break;
                }
                i -= 1;
                if digits[i] == 9 {
                    digits[i] = 0;
                } else {
                    digits[i] += 1;
                    break;
                }
            }
        }
    } else {
        digits.resize(keep, 0);
    }
    let mut mantissa = String::with_capacity(keep + 2);
    mantissa.push((digits[0] + b'0') as char);
    if dp > 0 {
        mantissa.push('.');
        for d in &digits[1..] {
            mantissa.push((d + b'0') as char);
        }
    }
    let sign = if neg { "-" } else { "" };
    let esign = if exp >= 0 { "+" } else { "-" };
    format!("{sign}{mantissa}e{esign}{}", exp.abs())
}

/// `Number.prototype.toFixed` / `toPrecision` FIXED-notation digit generation
/// with ECMA-262 rounding (round half away from zero on a tie). `value` is
/// finite with `|value| < 1e21`; `dp` is the fraction-digit count.
///
/// Rounds the TRUE IEEE-754 value via an exact ≤1100-digit expansion, so both a
/// genuine tie (`(1.25).toPrecision(2)` → `1.3`, `(1000000000000000.5).toFixed(0)`
/// → `…001`) AND a precision artifact (`(0.015).toFixed(2)` → `0.01`, because the
/// stored double is `0.01499…`) resolve on the real value — matching V8. Replaces
/// Rust's `format!("{:.N}")`, which rounds half-to-even (banker's rounding).
/// Number of fractional decimal digits in the EXACT decimal expansion of a
/// finite `x >= 0`.
///
/// A finite double is `m * 2^e` with `m` an odd integer. For `e >= 0` that is
/// an integer, so zero fractional digits; for `e < 0` it is
/// `m * 5^(-e) / 10^(-e)`, i.e. EXACTLY `-e` fractional digits and no more.
/// The worst case is 1074, for the smallest subnormal - and that worst case is
/// the only reason [`spec_to_fixed`] asked `format!` for 1100 places on every
/// input, including `6.0`, which needs none.
///
/// Over-asking is harmless (the extra places are zeros); under-asking is not,
/// because the manual round-half-up in `spec_to_fixed` is correct only while
/// the expansion it reads is exact rather than itself rounded. Callers take
/// the MAX of this and `dp + 1`, which keeps the expansion exact AND keeps
/// that function's invariant that the fraction string is at least `dp + 1`
/// long (it indexes `frac[dp]` to decide the rounding).
fn exact_fraction_digits(x: f64) -> usize {
    let bits = x.to_bits();
    let biased = ((bits >> 52) & 0x7FF) as i32;
    let mantissa = bits & 0x000F_FFFF_FFFF_FFFF;
    // Subnormals carry no implicit leading 1 and a fixed exponent; normals
    // take the implicit bit and the 1075 = 1023 bias + 52 mantissa-bit shift.
    let (m, e) = if biased == 0 {
        (mantissa, -1074i32)
    } else {
        (mantissa | (1u64 << 52), biased - 1075)
    };
    if m == 0 {
        return 0;
    }
    // Normalize `m` to odd: each trailing zero bit is a factor of two that
    // belongs in the exponent. This is what makes `6.0` cost 0 rather than 50.
    let e = e + m.trailing_zeros() as i32;
    if e >= 0 {
        0
    } else {
        (-e) as usize
    }
}

fn spec_to_fixed(value: f64, dp: usize) -> String {
    let neg = value.is_sign_negative() && value != 0.0;
    let x = value.abs();
    // Exact expansion, but only as long as THIS value actually is. 1100 places
    // covers the smallest subnormal, which is the worst case in the whole
    // domain and nothing like the common one: `(6.0).toFixed(7)` expanded to
    // 1100 decimal places and discarded 1093 of them. The expansion runs
    // through `flt2dec`'s dragon4 with a `Big32x40` bignum, so that is real
    // work - 6,927 Ir/op against 646 for `toFixed(6)`, an 11x step for one
    // more decimal place (#10770).
    //
    // `prec >= exact_fraction_digits(x)` keeps the expansion EXACT, so the
    // manual round-half-up below still reads true digits, and `>= dp + 1`
    // keeps `frac_str[dp]` in range. Mirrors `spec_to_exponential`, which
    // still uses the fixed 1100.
    let prec = exact_fraction_digits(x).max(dp + 1).min(1100);
    let full = format!("{x:.prec$}");
    let dot = full.find('.').unwrap_or(full.len());
    let int_str = &full[..dot];
    let frac_str = full.get(dot + 1..).unwrap_or("");
    let mut digits: Vec<u8> = Vec::with_capacity(int_str.len() + dp + 1);
    for b in int_str.bytes() {
        digits.push(b - b'0');
    }
    for b in frac_str.bytes().take(dp) {
        digits.push(b - b'0');
    }
    let mut int_len = int_str.len();
    // Round half away from zero: the first dropped fraction digit decides.
    if frac_str.as_bytes().get(dp).is_some_and(|&b| b >= b'5') {
        let mut i = digits.len();
        loop {
            if i == 0 {
                // Carry past the most significant digit (`9.99…` → `10.0…`).
                digits.insert(0, 1);
                int_len += 1;
                break;
            }
            i -= 1;
            if digits[i] == 9 {
                digits[i] = 0;
            } else {
                digits[i] += 1;
                break;
            }
        }
    }
    let mut out = String::with_capacity(digits.len() + 2);
    if neg {
        out.push('-');
    }
    for &d in &digits[..int_len] {
        out.push((d + b'0') as char);
    }
    if dp > 0 {
        out.push('.');
        for &d in &digits[int_len..] {
            out.push((d + b'0') as char);
        }
    }
    out
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

/// Format a number per JS toString rules (helper for toPrecision with no precision).
fn format_number_for_js(value: f64) -> String {
    js_format_f64(value)
}
