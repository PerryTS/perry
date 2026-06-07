//! `Temporal.Duration` — wraps [`temporal_rs::Duration`] (#4688).
//!
//! The shared return type of every `add`/`subtract`/`until`/`since` on the
//! other Temporal types, so it lands first. A Duration is a fixed length of
//! time as a tuple of calendar + clock fields; all the arithmetic lives in
//! `temporal_rs`, this module is marshalling glue.

use super::dispatch::{
    self, boolean, int_arg, is_undefined, number_i128, ok_or_throw, raw_arg, string,
};
use super::{alloc_temporal_cell, temporal_value_ref, TemporalValue};
use crate::value::JSValue;
use temporal_rs::Duration;

const TYPE_NAME: &str = "Temporal.Duration";

/// Field names in `Temporal.Duration` constructor / `from(object)` order.
const FIELD_NAMES: [&str; 10] = [
    "years",
    "months",
    "weeks",
    "days",
    "hours",
    "minutes",
    "seconds",
    "milliseconds",
    "microseconds",
    "nanoseconds",
];

/// Wrap a `temporal_rs::Duration` in a fresh Temporal cell. `pub(crate)` so the
/// other types' `add`/`subtract`/`until`/`since` (which all return a Duration)
/// can box their results.
pub(crate) fn wrap(d: Duration) -> f64 {
    alloc_temporal_cell(TemporalValue::Duration(d))
}

/// `new Temporal.Duration(years?, months?, …, nanoseconds?)` — every argument
/// is an optional integer defaulting to 0.
pub fn construct(args: &[f64]) -> f64 {
    let d = ok_or_throw(Duration::new(
        int_arg(args, 0),         // years
        int_arg(args, 1),         // months
        int_arg(args, 2),         // weeks
        int_arg(args, 3),         // days
        int_arg(args, 4),         // hours
        int_arg(args, 5),         // minutes
        int_arg(args, 6),         // seconds
        int_arg(args, 7),         // milliseconds
        int_arg(args, 8) as i128, // microseconds
        int_arg(args, 9) as i128, // nanoseconds
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
    // Object → read each duration field (missing = 0).
    if jv.is_pointer() {
        let obj = jv.as_pointer::<crate::object::ObjectHeader>();
        if !obj.is_null() {
            let f = |name: &str| -> i64 {
                let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                let raw = crate::object::js_object_get_field_by_name_f64(obj, key);
                let n = JSValue::from_bits(raw.to_bits()).to_number();
                if n.is_finite() {
                    n.trunc() as i64
                } else {
                    0
                }
            };
            return ok_or_throw(Duration::new(
                f("years"),
                f("months"),
                f("weeks"),
                f("days"),
                f("hours"),
                f("minutes"),
                f("seconds"),
                f("milliseconds"),
                f("microseconds") as i128,
                f("nanoseconds") as i128,
            ));
        }
    }
    crate::fs::validate::throw_range_error_with_code("Cannot convert value to a Temporal.Duration")
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
    let ord = ok_or_throw(a.compare(&b, None));
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
        "add" => wrap(ok_or_throw(d.add(&coerce_duration(raw_arg(args, 0))))),
        "subtract" => wrap(ok_or_throw(d.subtract(&coerce_duration(raw_arg(args, 0))))),
        "with" => with(d, raw_arg(args, 0)),
        "toString" | "toJSON" | "toLocaleString" => string(&super::temporal_value_iso_string(
            &TemporalValue::Duration(*d),
        )),
        "valueOf" => dispatch::throw_value_of(TYPE_NAME),
        "round" => {
            let opts = super::options::rounding_options(raw_arg(args, 0));
            let rel = super::options::relative_to(raw_arg(args, 0));
            wrap(ok_or_throw(d.round(opts, rel)))
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
    let jv = JSValue::from_bits(partial.to_bits());
    if !jv.is_pointer() {
        crate::fs::validate::throw_range_error_with_code(
            "Temporal.Duration.prototype.with requires an object argument",
        );
    }
    let obj = jv.as_pointer::<crate::object::ObjectHeader>();
    if obj.is_null() {
        crate::fs::validate::throw_range_error_with_code(
            "Temporal.Duration.prototype.with requires an object argument",
        );
    }
    let current: [i128; 10] = [
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
    let mut next = current;
    for (i, fname) in FIELD_NAMES.iter().enumerate() {
        let key = crate::string::js_string_from_bytes(fname.as_ptr(), fname.len() as u32);
        let raw = crate::object::js_object_get_field_by_name_f64(obj, key);
        if !is_undefined(raw) {
            let n = JSValue::from_bits(raw.to_bits()).to_number();
            if n.is_finite() {
                next[i] = n.trunc() as i128;
            }
        }
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
