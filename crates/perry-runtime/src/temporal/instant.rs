//! `Temporal.Instant` — wraps [`temporal_rs::Instant`] (#4690).
//!
//! An exact point on the epoch timeline at nanosecond precision. The epoch
//! count is a `bigint` on the JS side (it exceeds `2^53`), so construction and
//! the `epochNanoseconds` getter go through the BigInt marshalling helpers.

use super::dispatch::{self, bigint_from_i128, ok_or_throw, raw_arg, read_bigint_i128, string};
use super::{alloc_temporal_cell, temporal_value_ref, TemporalValue};
use crate::value::JSValue;
use temporal_rs::options::{DifferenceSettings, ToStringRoundingOptions};
use temporal_rs::Instant;

const TYPE_NAME: &str = "Temporal.Instant";

fn wrap(i: Instant) -> f64 {
    alloc_temporal_cell(TemporalValue::Instant(i))
}

fn require_ns(v: f64) -> i128 {
    match read_bigint_i128(v) {
        Some(n) => n,
        None => crate::fs::validate::throw_range_error_with_code(
            "Temporal.Instant requires a BigInt epoch-nanoseconds value",
        ),
    }
}

/// `new Temporal.Instant(epochNanoseconds: bigint)`.
pub fn construct(args: &[f64]) -> f64 {
    wrap(ok_or_throw(Instant::try_new(require_ns(raw_arg(args, 0)))))
}

fn coerce_instant(v: f64) -> Instant {
    if let Some(TemporalValue::Instant(i)) = temporal_value_ref(v) {
        return *i;
    }
    let jv = JSValue::from_bits(v.to_bits());
    if jv.is_string() {
        return ok_or_throw(Instant::from_utf8(dispatch::read_string(v).as_bytes()));
    }
    if jv.is_bigint() {
        return ok_or_throw(Instant::try_new(require_ns(v)));
    }
    crate::fs::validate::throw_range_error_with_code("Cannot convert value to a Temporal.Instant")
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
            DifferenceSettings::default(),
        ))),
        "since" => super::duration::wrap(ok_or_throw(i.since(
            &coerce_instant(raw_arg(args, 0)),
            DifferenceSettings::default(),
        ))),
        "equals" => dispatch::boolean(i.as_i128() == coerce_instant(raw_arg(args, 0)).as_i128()),
        "toString" | "toJSON" | "toLocaleString" => string(
            &i.to_ixdtf_string(None, ToStringRoundingOptions::default())
                .unwrap_or_default(),
        ),
        "valueOf" => dispatch::throw_value_of(TYPE_NAME),
        "round" | "toZonedDateTimeISO" => crate::fs::validate::throw_range_error_with_code(
            "Temporal.Instant.prototype.round/toZonedDateTimeISO is not yet implemented in Perry",
        ),
        _ => {
            let _ = recv;
            dispatch::throw_no_method(TYPE_NAME, name)
        }
    }
}
