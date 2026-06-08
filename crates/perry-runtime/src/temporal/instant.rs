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
        return t.parse::<i128>().unwrap_or_else(|_| {
            crate::fs::validate::throw_range_error_with_code("Cannot convert string to a BigInt")
        });
    }
    crate::object::throw_object_type_error(
        b"Cannot convert value to a BigInt for Temporal.Instant epoch-nanoseconds",
    )
}

/// `new Temporal.Instant(epochNanoseconds: bigint)`.
pub fn construct(args: &[f64]) -> f64 {
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
    let ms = JSValue::from_bits(raw_arg(args, 0).to_bits()).to_number();
    wrap(ok_or_throw(Instant::from_epoch_milliseconds(ms as i64)))
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
        "toJSON" | "toLocaleString" => string(
            &i.to_ixdtf_string(None, ToStringRoundingOptions::default())
                .unwrap_or_default(),
        ),
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
