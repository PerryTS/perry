//! `Temporal.Duration` — wraps [`temporal_rs::Duration`] (#4688).
//!
//! The shared return type of every `add`/`subtract`/`until`/`since` on the
//! other Temporal types, so it lands first. A Duration is a fixed length of
//! time as a tuple of calendar + clock fields; all the arithmetic lives in
//! `temporal_rs`, this module is marshalling glue.

use super::dispatch::{
    self, boolean, integral_arg, is_undefined, number_i128, ok_or_throw, raw_arg, string,
    to_integer_if_integral,
};
use super::{alloc_temporal_cell, temporal_value_ref, TemporalValue};
use crate::value::JSValue;
use temporal_rs::Duration;

const TYPE_NAME: &str = "Temporal.Duration";

/// Wrap a `temporal_rs::Duration` in a fresh Temporal cell. `pub(crate)` so the
/// other types' `add`/`subtract`/`until`/`since` (which all return a Duration)
/// can box their results.
pub(crate) fn wrap(d: Duration) -> f64 {
    alloc_temporal_cell(TemporalValue::Duration(d))
}

/// Round each Duration field to its nearest float64-representable integer.
/// The ECMAScript spec coerces Duration fields through ℝ(𝔽(·)) after arithmetic
/// (add, subtract, round) so the stored value never carries sub-float64 precision
/// that JS Numbers cannot represent. temporal_rs works in i128, so without this
/// step `toString()` and subsequent arithmetic would diverge from the spec.
fn f64_round_duration(d: Duration) -> Duration {
    ok_or_throw(Duration::new(
        (d.years() as f64) as i64,
        (d.months() as f64) as i64,
        (d.weeks() as f64) as i64,
        (d.days() as f64) as i64,
        (d.hours() as f64) as i64,
        (d.minutes() as f64) as i64,
        (d.seconds() as f64) as i64,
        (d.milliseconds() as f64) as i64,
        (d.microseconds() as f64) as i128,
        (d.nanoseconds() as f64) as i128,
    ))
}

/// `new Temporal.Duration(years?, months?, …, nanoseconds?)` — every argument
/// is an optional integer defaulting to 0.
pub fn construct(args: &[f64]) -> f64 {
    dispatch::require_construct(TYPE_NAME);
    let d = ok_or_throw(Duration::new(
        integral_arg(args, 0) as i64, // years
        integral_arg(args, 1) as i64, // months
        integral_arg(args, 2) as i64, // weeks
        integral_arg(args, 3) as i64, // days
        integral_arg(args, 4) as i64, // hours
        integral_arg(args, 5) as i64, // minutes
        integral_arg(args, 6) as i64, // seconds
        integral_arg(args, 7) as i64, // milliseconds
        integral_arg(args, 8),        // microseconds
        integral_arg(args, 9),        // nanoseconds
    ));
    wrap(d)
}

/// Coerce a JS value to a `temporal_rs::Duration` for `from` / `add` / etc.:
/// an existing `Temporal.Duration` is cloned, a string is parsed (ISO-8601
/// duration), and a plain object has its `years…nanoseconds` fields read.
/// `pub(crate)` so the other types' `add`/`subtract` can accept a Duration arg
/// in any of these forms.
pub(crate) fn coerce_duration(v: f64) -> Duration {
    // Already a Temporal.Duration → clone its value.
    if let Some(TemporalValue::Duration(d)) = temporal_value_ref(v) {
        return *d;
    }
    let jv = JSValue::from_bits(v.to_bits());
    // String → ISO-8601 duration parse.
    if jv.is_string() {
        let s = super::dispatch::read_string(v);
        return ok_or_throw(Duration::from_utf8(s.as_bytes()));
    }
    // Object → read each duration field (`ToTemporalDurationRecord`). A missing
    // field defaults to 0, a present one goes through `ToIntegerIfIntegral`
    // (RangeError on a fractional / infinite value), and an object carrying
    // *no* recognized duration field at all is a TypeError.
    if jv.is_pointer() {
        let obj = jv.as_pointer::<crate::object::ObjectHeader>();
        if !obj.is_null() {
            let mut vals = [0i128; 10];
            let mut any = false;
            // Spec `ToTemporalDurationRecord` reads the fields in *alphabetical*
            // order (days, hours, microseconds, …), each immediately coerced via
            // ToNumber — the order-of-operations tests observe this exact
            // interleaving. `slot` maps each name back to its positional index.
            const ALPHA_FIELDS: [(&str, usize); 10] = [
                ("days", 3),
                ("hours", 4),
                ("microseconds", 8),
                ("milliseconds", 7),
                ("minutes", 5),
                ("months", 1),
                ("nanoseconds", 9),
                ("seconds", 6),
                ("weeks", 2),
                ("years", 0),
            ];
            for (name, slot) in ALPHA_FIELDS {
                let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                let raw = crate::object::js_object_get_field_by_name_f64(obj, key);
                if !is_undefined(raw) {
                    any = true;
                    vals[slot] = to_integer_if_integral(raw);
                }
            }
            if !any {
                crate::object::throw_object_type_error(
                    b"Temporal.Duration-like object must have at least one duration property",
                );
            }
            return ok_or_throw(Duration::new(
                vals[0] as i64,
                vals[1] as i64,
                vals[2] as i64,
                vals[3] as i64,
                vals[4] as i64,
                vals[5] as i64,
                vals[6] as i64,
                vals[7] as i64,
                vals[8],
                vals[9],
            ));
        }
    }
    // A non-object, non-string primitive (number, boolean, bigint, symbol,
    // undefined, null) is never a Duration — `ToTemporalDuration` throws a
    // TypeError before the string-parse step.
    crate::object::throw_object_type_error(b"Cannot convert value to a Temporal.Duration")
}

// ---- statics (installed on the constructor) -------------------------------

/// `Temporal.Duration.from(thing)`.
pub fn from_static(args: &[f64]) -> f64 {
    wrap(coerce_duration(raw_arg(args, 0)))
}

/// `Temporal.Duration.compare(a, b)` → -1 | 0 | 1. (Calendar-relative
/// comparison via the `relativeTo` option is deferred; bare durations compare
/// by normalized total nanoseconds, which `temporal_rs` does when no
/// `relativeTo` is needed.)
pub fn compare_static(args: &[f64]) -> f64 {
    let a = coerce_duration(raw_arg(args, 0));
    let b = coerce_duration(raw_arg(args, 1));
    // The `{ relativeTo }` option anchors calendar-unit (years/months/weeks)
    // comparison; the options bag itself must be an object or undefined
    // (TypeError otherwise), and a malformed `relativeTo` string is a RangeError
    // — both before the early-equal return.
    super::options::require_options_object(raw_arg(args, 2));
    let rel = super::options::relative_to(raw_arg(args, 2));
    let ord = ok_or_throw(a.compare(&b, rel));
    match ord {
        std::cmp::Ordering::Less => -1.0,
        std::cmp::Ordering::Equal => 0.0,
        std::cmp::Ordering::Greater => 1.0,
    }
}

// ---- getters --------------------------------------------------------------

/// Sign without leaning on `temporal_rs::Sign`'s variant names: a valid
/// Duration has a single sign across all fields, so the first non-zero field's
/// sign is the Duration's sign.
fn sign(d: &Duration) -> f64 {
    let fields: [i128; 10] = [
        d.years() as i128,
        d.months() as i128,
        d.weeks() as i128,
        d.days() as i128,
        d.hours() as i128,
        d.minutes() as i128,
        d.seconds() as i128,
        d.milliseconds() as i128,
        d.microseconds(),
        d.nanoseconds(),
    ];
    for v in fields {
        if v != 0 {
            return if v > 0 { 1.0 } else { -1.0 };
        }
    }
    0.0
}

pub fn get(d: &Duration, name: &str) -> Option<f64> {
    Some(match name {
        "years" => d.years() as f64,
        "months" => d.months() as f64,
        "weeks" => d.weeks() as f64,
        "days" => d.days() as f64,
        "hours" => d.hours() as f64,
        "minutes" => d.minutes() as f64,
        "seconds" => d.seconds() as f64,
        "milliseconds" => d.milliseconds() as f64,
        "microseconds" => number_i128(d.microseconds()),
        "nanoseconds" => number_i128(d.nanoseconds()),
        "sign" => sign(d),
        "blank" => boolean(d.is_zero()),
        _ => return None,
    })
}

// ---- methods --------------------------------------------------------------

pub fn call(recv: f64, d: &Duration, name: &str, args: &[f64]) -> f64 {
    match name {
        "negated" => wrap(d.negated()),
        "abs" => wrap(d.abs()),
        "add" => wrap(f64_round_duration(ok_or_throw(
            d.add(&coerce_duration(raw_arg(args, 0))),
        ))),
        "subtract" => wrap(f64_round_duration(ok_or_throw(
            d.subtract(&coerce_duration(raw_arg(args, 0))),
        ))),
        "with" => with(d, raw_arg(args, 0)),
        // `toString` honors a `{ fractionalSecondDigits, smallestUnit,
        // roundingMode }` options bag; `toJSON`/`toLocaleString` always use the
        // default precision.
        "toString" => string(&ok_or_throw(d.as_temporal_string(
            super::options::to_string_rounding_options(raw_arg(args, 0)),
        ))),
        "toJSON" | "toLocaleString" => string(&super::temporal_value_iso_string(
            &TemporalValue::Duration(*d),
        )),
        "valueOf" => dispatch::throw_value_of(TYPE_NAME),
        "round" => {
            let (opts, rel) = super::options::duration_round_options(raw_arg(args, 0));
            wrap(f64_round_duration(ok_or_throw(d.round(opts, rel))))
        }
        "total" => {
            let (unit, rel) = super::options::total_options(raw_arg(args, 0));
            ok_or_throw(d.total(unit, rel)).as_inner()
        }
        _ => {
            let _ = recv;
            dispatch::throw_no_method(TYPE_NAME, name)
        }
    }
}

/// `duration.with({ hours: 3, … })` — return a copy with the supplied fields
/// replaced. Reads each provided field from the partial object; unspecified
/// fields keep the receiver's value.
fn with(d: &Duration, partial: f64) -> f64 {
    // `ToTemporalPartialDurationRecord` requires an Object — a primitive
    // (`undefined`, number, string, …) is a `TypeError`, not a RangeError. A
    // Symbol is a NaN-boxed pointer but is also rejected.
    let jv = JSValue::from_bits(partial.to_bits());
    if !jv.is_pointer() || unsafe { crate::symbol::js_is_symbol(partial) } != 0 {
        crate::object::throw_object_type_error(
            b"Temporal.Duration.prototype.with requires an object argument",
        );
    }
    let obj = jv.as_pointer::<crate::object::ObjectHeader>();
    if obj.is_null() {
        crate::object::throw_object_type_error(
            b"Temporal.Duration.prototype.with requires an object argument",
        );
    }
    let mut next: [i128; 10] = [
        d.years() as i128,
        d.months() as i128,
        d.weeks() as i128,
        d.days() as i128,
        d.hours() as i128,
        d.minutes() as i128,
        d.seconds() as i128,
        d.milliseconds() as i128,
        d.microseconds(),
        d.nanoseconds(),
    ];
    // `ToTemporalPartialDurationRecord` reads the fields in *alphabetical* order
    // (the order-of-operations tests observe this), each coerced via
    // `ToIntegerIfIntegral` — a fractional / infinite value is a RangeError and a
    // Symbol / BigInt a TypeError (not a silent drop). At least one recognized
    // field must be present, else TypeError.
    const ALPHA_FIELDS: [(&str, usize); 10] = [
        ("days", 3),
        ("hours", 4),
        ("microseconds", 8),
        ("milliseconds", 7),
        ("minutes", 5),
        ("months", 1),
        ("nanoseconds", 9),
        ("seconds", 6),
        ("weeks", 2),
        ("years", 0),
    ];
    let mut any = false;
    for (fname, slot) in ALPHA_FIELDS {
        let key = crate::string::js_string_from_bytes(fname.as_ptr(), fname.len() as u32);
        let raw = crate::object::js_object_get_field_by_name_f64(obj, key);
        if !is_undefined(raw) {
            any = true;
            next[slot] = to_integer_if_integral(raw);
        }
    }
    if !any {
        crate::object::throw_object_type_error(
            b"Temporal.Duration.prototype.with requires at least one duration field",
        );
    }
    wrap(ok_or_throw(Duration::new(
        next[0] as i64,
        next[1] as i64,
        next[2] as i64,
        next[3] as i64,
        next[4] as i64,
        next[5] as i64,
        next[6] as i64,
        next[7] as i64,
        next[8],
        next[9],
    )))
}
