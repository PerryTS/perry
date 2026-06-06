//! `Temporal.Now` — wraps [`temporal_rs::Temporal::local_now`] (#4689).
//!
//! A namespace (not a constructor), like `Math`: a plain object of method
//! thunks. Each call reads the host clock fresh via `Temporal::local_now()`
//! (the `sys-local` feature supplies the system time zone + clock).

use super::dispatch::{self, ok_or_throw, raw_arg, string};
use super::{alloc_temporal_cell, TemporalValue};
use crate::value::JSValue;
use temporal_rs::{Temporal, TimeZone};

/// Resolve an optional time-zone argument (an IANA id string) to a `TimeZone`,
/// or `None` to use the host's current zone.
fn tz_arg(v: f64) -> Option<TimeZone> {
    if dispatch::is_undefined(v) {
        return None;
    }
    let jv = JSValue::from_bits(v.to_bits());
    if jv.is_string() {
        let s = dispatch::read_string(v);
        return Some(ok_or_throw(TimeZone::try_from_str(&s)));
    }
    None
}

pub fn instant(_args: &[f64]) -> f64 {
    alloc_temporal_cell(TemporalValue::Instant(ok_or_throw(
        Temporal::local_now().instant(),
    )))
}

pub fn time_zone_id(_args: &[f64]) -> f64 {
    let tz = ok_or_throw(Temporal::local_now().time_zone());
    string(&ok_or_throw(tz.identifier()))
}

pub fn plain_date_time_iso(args: &[f64]) -> f64 {
    let tz = tz_arg(raw_arg(args, 0));
    alloc_temporal_cell(TemporalValue::PlainDateTime(ok_or_throw(
        Temporal::local_now().plain_date_time_iso(tz),
    )))
}

pub fn plain_date_iso(args: &[f64]) -> f64 {
    let tz = tz_arg(raw_arg(args, 0));
    alloc_temporal_cell(TemporalValue::PlainDate(ok_or_throw(
        Temporal::local_now().plain_date_iso(tz),
    )))
}

pub fn plain_time_iso(args: &[f64]) -> f64 {
    let tz = tz_arg(raw_arg(args, 0));
    alloc_temporal_cell(TemporalValue::PlainTime(ok_or_throw(
        Temporal::local_now().plain_time_iso(tz),
    )))
}

pub fn zoned_date_time_iso(args: &[f64]) -> f64 {
    let tz = tz_arg(raw_arg(args, 0));
    alloc_temporal_cell(TemporalValue::ZonedDateTime(ok_or_throw(
        Temporal::local_now().zoned_date_time_iso(tz),
    )))
}
