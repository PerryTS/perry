//! `Temporal.Now` — a namespace of method thunks reading the host clock/zone
//!
//! A namespace (not a constructor), like `Math`: a plain object of method
//! thunks. Each call reads the host clock fresh via `perry_now()`, backed by
//! [`PerryHostSystem`] — perry's own host system (std `SystemTime` clock,
//! `crate::date::host_time_zone_name` for the zone). We do NOT use temporal_rs's
//! `sys-local` feature, which would pull `iana_time_zone` (CoreFoundation).

use super::dispatch::{self, ok_or_throw, raw_arg, string};
use super::{alloc_temporal_cell, TemporalValue};
use temporal_rs::host::{HostClock, HostHooks, HostTimeZone};
use temporal_rs::provider::TimeZoneProvider;
use temporal_rs::unix_time::EpochNanoseconds;
use temporal_rs::now::Now;
use temporal_rs::{TemporalError, TemporalResult, TimeZone};

/// Perry's own `Temporal.Now` host system, replacing temporal_rs's
/// `LocalHostSystem` (its `sys-local` feature). `LocalHostSystem` resolved the
/// system zone via `iana_time_zone::get_timezone`, which links CoreFoundation on
/// macOS — and because Temporal's namespace is registered in the always-linked
/// runtime init, that CF dependency was dragged into EVERY output binary,
/// forcing `-framework CoreFoundation` even on otherwise libSystem-only
/// runtime-only programs (they'd fail to link with undefined `_CFRelease` etc.).
/// Perry already resolves the host zone itself — `crate::date::host_time_zone_name`
/// via `TZ` / `/etc/localtime`, no CF — so we drop `sys-local` and feed the zone
/// and clock through temporal_rs's public `HostHooks` traits. Net: no binary
/// links CoreFoundation for time zones; `Temporal.Now.*` is unchanged.
struct PerryHostSystem;

impl HostClock for PerryHostSystem {
    fn get_host_epoch_nanoseconds(&self) -> TemporalResult<EpochNanoseconds> {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| TemporalError::general("Error fetching system time"))
            .map(|d| EpochNanoseconds::from(d.as_nanos() as i128))
    }
}

impl HostTimeZone for PerryHostSystem {
    fn get_host_time_zone(
        &self,
        provider: &(impl TimeZoneProvider + ?Sized),
    ) -> TemporalResult<TimeZone> {
        TimeZone::try_from_identifier_str_with_provider(crate::date::host_time_zone_name(), provider)
    }
}

impl HostHooks for PerryHostSystem {}

/// `Temporal.Now` for perry's host system (drop-in for the removed
/// `temporal_rs::Temporal::local_now()`)..
#[inline]
fn perry_now() -> Now<PerryHostSystem> {
    Now::new(PerryHostSystem)
}

/// Resolve an optional time-zone argument (an IANA id string or a
/// `Temporal.ZonedDateTime`) to a `TimeZone`, or `None` (absent / `undefined`)
/// to use the host's current zone. A present-but-wrong-typed value is rejected
/// exactly like `ToTemporalTimeZoneIdentifier`: an invalid string → `RangeError`,
/// any non-string / non-ZonedDateTime primitive or object → `TypeError`. (The
/// old version silently dropped wrong types to `None`, so the host zone was used
/// instead of throwing — the `timezone-wrong-type` cases never rejected.)
fn tz_arg(v: f64) -> Option<TimeZone> {
    if dispatch::is_undefined(v) {
        return None;
    }
    Some(super::options::timezone(v))
}

pub fn instant(_args: &[f64]) -> f64 {
    alloc_temporal_cell(TemporalValue::Instant(ok_or_throw(
        perry_now().instant(),
    )))
}

pub fn time_zone_id(_args: &[f64]) -> f64 {
    let tz = ok_or_throw(perry_now().time_zone());
    string(&ok_or_throw(tz.identifier()))
}

pub fn plain_date_time_iso(args: &[f64]) -> f64 {
    let tz = tz_arg(raw_arg(args, 0));
    alloc_temporal_cell(TemporalValue::PlainDateTime(ok_or_throw(
        perry_now().plain_date_time_iso(tz),
    )))
}

pub fn plain_date_iso(args: &[f64]) -> f64 {
    let tz = tz_arg(raw_arg(args, 0));
    alloc_temporal_cell(TemporalValue::PlainDate(ok_or_throw(
        perry_now().plain_date_iso(tz),
    )))
}

pub fn plain_time_iso(args: &[f64]) -> f64 {
    let tz = tz_arg(raw_arg(args, 0));
    alloc_temporal_cell(TemporalValue::PlainTime(ok_or_throw(
        perry_now().plain_time_iso(tz),
    )))
}

pub fn zoned_date_time_iso(args: &[f64]) -> f64 {
    let tz = tz_arg(raw_arg(args, 0));
    alloc_temporal_cell(TemporalValue::ZonedDateTime(ok_or_throw(
        perry_now().zoned_date_time_iso(tz),
    )))
}
