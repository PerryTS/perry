//! `Temporal.Instant` — wraps [`temporal_rs::Instant`] (#4690).
//!
//! An exact point on the epoch timeline at nanosecond precision. The epoch
//! count is a `bigint` on the JS side (it exceeds `2^53`), so construction and
//! the `epochNanoseconds` getter go through the BigInt marshalling helpers.

use super::dispatch::{self, bigint_from_i128, ok_or_throw, raw_arg, read_bigint_i128, string};
use super::{alloc_temporal_cell, temporal_value_ref, TemporalValue};
use crate::value::JSValue;
use temporal_rs::options::ToStringRoundingOptions;
use temporal_rs::Instant;

const TYPE_NAME: &str = "Temporal.Instant";

fn wrap(i: Instant) -> f64 {
    alloc_temporal_cell(TemporalValue::Instant(i))
}

/// `ToBigInt(value)` → `i128` for the epoch-nanoseconds slot of
/// `new Temporal.Instant` / `fromEpochNanoseconds`. A BigInt is taken
/// directly, a string is parsed (`StringToBigInt`), a boolean maps to 0/1, and
/// everything else (number, undefined, null, symbol) is a `TypeError` — Number
/// must go through `BigInt(...)` first, per spec.
fn require_ns(v: f64) -> i128 {
    let jv = JSValue::from_bits(v.to_bits());
    if jv.is_bigint() {
        return read_bigint_i128(v).unwrap_or_else(|| {
            crate::fs::validate::throw_range_error_with_code("Invalid BigInt value")
        });
    }
    if jv.is_bool() {
        return if v.to_bits() == crate::value::TAG_TRUE {
            1
        } else {
            0
        };
    }
    if jv.is_string() {
        let s = dispatch::read_string(v);
        let t = s.trim();
        if t.is_empty() {
            return 0;
        }
        // `StringToBigInt` failure is a **SyntaxError** (`new Temporal.Instant("abc123")`),
        // not a RangeError — invalid BigInt *syntax* is a parse error. A
        // syntactically-valid but out-of-i128-range string still rejects below
        // (and `Instant::try_new` then range-checks the value itself).
        return t.parse::<i128>().unwrap_or_else(|_| throw_bigint_syntax());
    }
    crate::object::throw_object_type_error(
        b"Cannot convert value to a BigInt for Temporal.Instant epoch-nanoseconds",
    )
}

/// Throw a JS `SyntaxError` for a string that is not valid BigInt syntax
/// (`StringToBigInt` failure), matching Node's `new Temporal.Instant("abc123")`.
fn throw_bigint_syntax() -> ! {
    let msg = b"Cannot convert string to a BigInt";
    let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err_ptr = crate::error::js_syntaxerror_new(msg_str);
    crate::exception::js_throw(f64::from_bits(
        JSValue::pointer(err_ptr as *const u8).bits(),
    ))
}

/// `ToNumber(epochMilliseconds)` then `NumberToBigInt`: a BigInt or Symbol is a
/// `TypeError` (abstract `ToNumber` rejects them), and a non-integral Number
/// (`Infinity`/`NaN`/`1.3`/`undefined`→NaN) is a `RangeError` (`NumberToBigInt`
/// requires an integral Number). `valueOf` is observed for objects.
fn epoch_number_to_integer(raw: f64) -> i64 {
    let jv = JSValue::from_bits(raw.to_bits());
    if jv.is_bigint() {
        crate::object::throw_object_type_error(b"Cannot convert a BigInt value to a number");
    }
    if unsafe { crate::symbol::js_is_symbol(raw) } != 0 {
        crate::object::throw_object_type_error(b"Cannot convert a Symbol value to a number");
    }
    let n = crate::builtins::js_number_coerce(raw);
    if !n.is_finite() || n.fract() != 0.0 {
        crate::fs::validate::throw_range_error_with_code(
            "epochMilliseconds must be an integral Number",
        );
    }
    n as i64
}

/// `new Temporal.Instant(epochNanoseconds: bigint)`.
pub fn construct(args: &[f64]) -> f64 {
    dispatch::require_construct(TYPE_NAME);
    wrap(ok_or_throw(Instant::try_new(require_ns(raw_arg(args, 0)))))
}

/// `ToTemporalInstant(item)` for `from` / `compare` / `equals` / `until` /
/// `since`. An existing `Temporal.Instant` is returned directly, a
/// `Temporal.ZonedDateTime` yields its exact instant, a string is parsed, and a
/// non-Temporal object is `ToString`-coerced then parsed. A bare BigInt /
/// Number / boolean is **not** valid here (only the constructor accepts an
/// epoch-nanoseconds value) — those throw a `TypeError`.
fn coerce_instant(v: f64) -> Instant {
    if let Some(tv) = temporal_value_ref(v) {
        match tv {
            TemporalValue::Instant(i) => return *i,
            TemporalValue::ZonedDateTime(z) => return z.to_instant(),
            // Any other Temporal type falls through to ToString + parse, which
            // will reject (a PlainDate string has no exact instant, etc.).
            _ => {}
        }
    }
    let jv = JSValue::from_bits(v.to_bits());
    if jv.is_string() {
        return ok_or_throw(Instant::from_utf8(dispatch::read_string(v).as_bytes()));
    }
    // Non-Temporal object → ToString → parse as an instant string.
    if jv.is_pointer() && unsafe { crate::symbol::js_is_symbol(v) } == 0 {
        let sh = crate::value::js_jsvalue_to_string_coerce(v);
        let s = dispatch::read_string(crate::value::js_nanbox_string(sh as i64));
        return ok_or_throw(Instant::from_utf8(s.as_bytes()));
    }
    crate::object::throw_object_type_error(b"Cannot convert value to a Temporal.Instant")
}

// ---- statics --------------------------------------------------------------

pub fn from_static(args: &[f64]) -> f64 {
    wrap(coerce_instant(raw_arg(args, 0)))
}

pub fn from_epoch_milliseconds_static(args: &[f64]) -> f64 {
    let ms = epoch_number_to_integer(raw_arg(args, 0));
    wrap(ok_or_throw(Instant::from_epoch_milliseconds(ms)))
}

pub fn from_epoch_nanoseconds_static(args: &[f64]) -> f64 {
    wrap(ok_or_throw(Instant::try_new(require_ns(raw_arg(args, 0)))))
}

pub fn compare_static(args: &[f64]) -> f64 {
    let a = coerce_instant(raw_arg(args, 0)).as_i128();
    let b = coerce_instant(raw_arg(args, 1)).as_i128();
    match a.cmp(&b) {
        std::cmp::Ordering::Less => -1.0,
        std::cmp::Ordering::Equal => 0.0,
        std::cmp::Ordering::Greater => 1.0,
    }
}

// ---- getters --------------------------------------------------------------

pub fn get(i: &Instant, name: &str) -> Option<f64> {
    Some(match name {
        "epochMilliseconds" => i.epoch_milliseconds() as f64,
        "epochNanoseconds" => bigint_from_i128(i.as_i128()),
        _ => return None,
    })
}

// ---- methods --------------------------------------------------------------

pub fn call(recv: f64, i: &Instant, name: &str, args: &[f64]) -> f64 {
    match name {
        "add" => wrap(ok_or_throw(
            i.add(&super::duration::coerce_duration(raw_arg(args, 0))),
        )),
        "subtract" => wrap(ok_or_throw(
            i.subtract(&super::duration::coerce_duration(raw_arg(args, 0))),
        )),
        "until" => super::duration::wrap(ok_or_throw(i.until(
            &coerce_instant(raw_arg(args, 0)),
            super::options::difference_settings(raw_arg(args, 1)),
        ))),
        "since" => super::duration::wrap(ok_or_throw(i.since(
            &coerce_instant(raw_arg(args, 0)),
            super::options::difference_settings(raw_arg(args, 1)),
        ))),
        "equals" => dispatch::boolean(i.as_i128() == coerce_instant(raw_arg(args, 0)).as_i128()),
        // `toString` honors `{ fractionalSecondDigits, smallestUnit,
        // roundingMode, timeZone }`; `toJSON`/`toLocaleString` use defaults.
        "toString" => {
            let opts = super::options::to_string_rounding_options(raw_arg(args, 0));
            let tz = super::options::optional_instant_timezone(raw_arg(args, 0));
            string(&ok_or_throw(i.to_ixdtf_string(tz, opts)))
        }
        "toJSON" => string(
            &i.to_ixdtf_string(None, ToStringRoundingOptions::default())
                .unwrap_or_default(),
        ),
        "toLocaleString" => {
            let epoch_ms = i.epoch_milliseconds() as f64;
            crate::intl::temporal_locale_string(
                epoch_ms,
                raw_arg(args, 0),
                raw_arg(args, 1),
                crate::intl::TemporalLocaleCtx::Instant,
            )
        }
        "valueOf" => dispatch::throw_value_of(TYPE_NAME),
        "round" => wrap(ok_or_throw(
            i.round(super::options::rounding_options(raw_arg(args, 0))),
        )),
        "toZonedDateTimeISO" => {
            let tz = super::options::timezone(raw_arg(args, 0));
            alloc_temporal_cell(TemporalValue::ZonedDateTime(ok_or_throw(
                i.to_zoned_date_time_iso(tz),
            )))
        }
        _ => {
            let _ = recv;
            dispatch::throw_no_method(TYPE_NAME, name)
        }
    }
}
