//! Shared civil-time arithmetic and host/Intl zone projection.
/// Convert a UTC timestamp (seconds) to local-time components.
/// Returns (year, month [1-12], day, hour, minute, second, tz_offset_seconds).
/// tz_offset_seconds is the number of seconds that need to be added to the
/// UTC timestamp to get the local-time representation (i.e. local - UTC).
#[cfg(unix)]
pub(crate) fn timestamp_to_local_components(secs: i64) -> (i32, u32, u32, u32, u32, u32, i64) {
    unsafe {
        let t: libc::time_t = secs as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        let res = libc::localtime_r(&t, &mut tm);
        if res.is_null() {
            let (y, m, d, h, mi, s) = timestamp_to_components(secs);
            return (y, m, d, h, mi, s, 0);
        }
        let year = tm.tm_year + 1900;
        let month = (tm.tm_mon + 1) as u32;
        let day = tm.tm_mday as u32;
        let hour = tm.tm_hour as u32;
        let minute = tm.tm_min as u32;
        let second = tm.tm_sec as u32;
        let tz_offset = tm.tm_gmtoff as i64;
        (year, month, day, hour, minute, second, tz_offset)
    }
}

/// WASI (#11377) has no time-zone database (wasi-libc's `localtime` is UTC),
/// so local time is UTC with a zero offset.
#[cfg(target_os = "wasi")]
pub(crate) fn timestamp_to_local_components(secs: i64) -> (i32, u32, u32, u32, u32, u32, i64) {
    let (y, m, d, h, mi, s) = timestamp_to_components(secs);
    (y, m, d, h, mi, s, 0)
}

#[cfg(windows)]
pub(crate) fn timestamp_to_local_components(secs: i64) -> (i32, u32, u32, u32, u32, u32, i64) {
    unsafe {
        let t: libc::time_t = secs as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        let err = libc::localtime_s(&mut tm, &t);
        if err != 0 {
            let (y, m, d, h, mi, s) = timestamp_to_components(secs);
            return (y, m, d, h, mi, s, 0);
        }
        let year = tm.tm_year + 1900;
        let month = (tm.tm_mon + 1) as u32;
        let day = tm.tm_mday as u32;
        let hour = tm.tm_hour as u32;
        let minute = tm.tm_min as u32;
        let second = tm.tm_sec as u32;
        let mut utm: libc::tm = std::mem::zeroed();
        let tz_offset = if libc::gmtime_s(&mut utm, &t) == 0 {
            let local_secs = components_to_timestamp(year, month, day, hour, minute, second);
            let utc_secs = components_to_timestamp(
                utm.tm_year + 1900,
                (utm.tm_mon + 1) as u32,
                utm.tm_mday as u32,
                utm.tm_hour as u32,
                utm.tm_min as u32,
                utm.tm_sec as u32,
            );
            local_secs - utc_secs
        } else {
            0
        };
        (year, month, day, hour, minute, second, tz_offset)
    }
}

/// The host IANA time-zone identifier (e.g. `"Europe/Berlin"`), matching what
/// Node returns from `Intl.DateTimeFormat().resolvedOptions().timeZone`. Honors
/// the `TZ` environment variable first (like libc / Node), then the OS default
/// via the `/etc/localtime` symlink; falls back to `"UTC"`. Cached for the
/// process lifetime (the host zone doesn't change under us).
pub fn host_time_zone_name() -> &'static str {
    use std::sync::OnceLock;
    static TZ: OnceLock<String> = OnceLock::new();
    crate::once_init::get_or_init(&TZ, || {
        if let Some(raw) = std::env::var_os("TZ") {
            if let Some(s) = raw.to_str() {
                // A `TZ` of `:Europe/Berlin` / `:/path/zoneinfo/Europe/Berlin`
                // (leading colon) or a bare IANA id. Empty `TZ` means UTC.
                let s = s.trim().strip_prefix(':').unwrap_or_else(|| s.trim());
                if s.is_empty() {
                    return "UTC".to_string();
                }
                if let Some(name) = iana_id_from_zoneinfo_path(s) {
                    return name;
                }
                return s.to_string();
            }
        }
        #[cfg(unix)]
        {
            if let Ok(target) = std::fs::read_link("/etc/localtime") {
                if let Some(name) = iana_id_from_zoneinfo_path(&target.to_string_lossy()) {
                    return name;
                }
            }
        }
        "UTC".to_string()
    })
    .as_str()
}

/// Extract the IANA id (`Europe/Berlin`) after a `.../zoneinfo/` path segment.
/// Returns `None` when the path has no `zoneinfo/` component (e.g. a bare id).
fn iana_id_from_zoneinfo_path(p: &str) -> Option<String> {
    const MARKER: &str = "zoneinfo/";
    p.rfind(MARKER).map(|i| p[i + MARKER.len()..].to_string())
}

/// Parse a fixed UTC offset like `"+02:00"`, `"-0500"`, `"+03"` into seconds
/// east of UTC. Returns `None` for named zones / non-offset strings.
fn parse_fixed_offset(tz: &str) -> Option<i64> {
    let bytes = tz.as_bytes();
    let sign = match bytes.first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let digits: String = tz[1..].chars().filter(|c| c.is_ascii_digit()).collect();
    let (h, m) = match digits.len() {
        1 | 2 => (digits.parse::<i64>().ok()?, 0),
        3 => (digits[..1].parse().ok()?, digits[1..].parse().ok()?),
        4 => (digits[..2].parse().ok()?, digits[2..].parse().ok()?),
        _ => return None,
    };
    if h > 23 || m > 59 {
        return None;
    }
    Some(sign * (h * 3600 + m * 60))
}

#[cfg(feature = "intl-datetime")]
fn compiled_tzdb() -> &'static timezone_provider::tzif::CompiledTzdbProvider {
    use std::sync::OnceLock;
    static PROVIDER: OnceLock<timezone_provider::tzif::CompiledTzdbProvider> = OnceLock::new();
    PROVIDER.get_or_init(Default::default)
}

/// Resolve a named zone through the compiled IANA database and return its
/// canonical identifier. This is also the membership check used by
/// `Intl.DateTimeFormat`: a structurally plausible but unknown name must not be
/// reported from `resolvedOptions()` as though Perry can format it.
#[cfg(feature = "intl-datetime")]
pub(crate) fn canonicalize_tzdb_name(tz: &str) -> Option<String> {
    use timezone_provider::provider::TimeZoneProvider;

    let provider = compiled_tzdb();
    let id = provider.get(tz.as_bytes()).ok()?;
    let canonical = provider.canonicalized(id).ok()?;
    provider
        .identifier(canonical)
        .ok()
        .map(|name| name.into_owned())
}

#[cfg(feature = "intl-datetime")]
fn compiled_zone_offset_seconds(tz: &str, secs: i64) -> Option<i64> {
    use timezone_provider::provider::TimeZoneProvider;

    let provider = compiled_tzdb();
    let id = provider.get(tz.as_bytes()).ok()?;
    let epoch_ns = i128::from(secs).checked_mul(1_000_000_000)?;
    provider
        .transition_nanoseconds_for_utc_epoch_nanoseconds(id, epoch_ns)
        .ok()
        .map(|offset| offset.0)
}

/// UTC offset (seconds east of UTC) for time-zone `tz` at instant `secs`,
/// DST-aware, matching the OS tz database — the amount to add to a UTC timestamp
/// to get the wall-clock time in `tz`. `UTC`/`GMT`/empty are 0; a fixed numeric
/// offset is parsed directly; the process's own host zone is read straight from
/// libc (thread-safe — it uses the process `TZ`). `intl-datetime` builds resolve
/// every other named zone through the compiled IANA database, without mutating
/// process-global state. Minimal builds without that feature retain the UTC
/// fallback for non-host named zones.
pub fn zone_offset_seconds(tz: &str, secs: i64) -> i64 {
    if tz.is_empty()
        || tz.eq_ignore_ascii_case("UTC")
        || tz.eq_ignore_ascii_case("GMT")
        || tz == "Etc/UTC"
        || tz == "Etc/GMT"
    {
        return 0;
    }
    if let Some(off) = parse_fixed_offset(tz) {
        return off;
    }
    if tz == host_time_zone_name() {
        // The process is already running in this zone; libc localtime applies
        // the correct (DST-aware) offset for `secs`.
        return timestamp_to_local_components(secs).6;
    }
    // The compiled IANA database is reached through the `intl-datetime`
    // install (see `crate::feature_hooks`); without it non-host named zones
    // keep the UTC fallback, as a build without the feature does.
    if let Some(offset) = COMPILED_ZONE_OFFSET.get().and_then(|f| f(tz, secs)) {
        return offset;
    }
    0
}

static COMPILED_ZONE_OFFSET: crate::feature_hooks::Hook<fn(&str, i64) -> Option<i64>> =
    crate::feature_hooks::Hook::empty();

/// The `intl-datetime` install's Date half.
#[cfg(feature = "intl-datetime")]
pub(crate) fn install_compiled_tzdb() {
    COMPILED_ZONE_OFFSET.set(compiled_zone_offset_seconds);
}

/// Convert date components (UTC) to Unix timestamp in seconds.
/// Inverse of timestamp_to_components. Used only by the Windows tz-offset
/// fallback; superseded elsewhere by [`make_utc_ms`] (which normalizes
/// out-of-range components).
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn components_to_timestamp(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
) -> i64 {
    // Howard Hinnant's civil_from_days (inverse of days_from_civil)
    let y = if month <= 2 {
        year as i64 - 1
    } else {
        year as i64
    };
    let m = if month <= 2 {
        month as i64 + 9
    } else {
        month as i64 - 3
    };
    let era = if y >= 0 { y / 400 } else { (y - 399) / 400 };
    let yoe = (y - era * 400) as u64;
    let doy = (153 * m as u64 + 2) / 5 + day as u64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe as i64 - 719468;

    days * 86400 + hour as i64 * 3600 + minute as i64 * 60 + second as i64
}

/// Convert Unix timestamp (seconds) to date components (year, month, day, hour, minute, second)
/// Returns components in UTC
pub fn timestamp_to_components(secs: i64) -> (i32, u32, u32, u32, u32, u32) {
    // Handle negative timestamps (dates before 1970)
    let is_negative = secs < 0;
    let abs_secs = if is_negative { -secs } else { secs } as u64;

    // Extract time of day
    let second = (abs_secs % 60) as u32;
    let minute = ((abs_secs / 60) % 60) as u32;
    let hour = ((abs_secs / 3600) % 24) as u32;

    // Calculate days from Unix epoch
    let days = if is_negative {
        -((abs_secs / 86400) as i64)
            - if !abs_secs.is_multiple_of(86400) {
                1
            } else {
                0
            }
    } else {
        (abs_secs / 86400) as i64
    };

    // For negative timestamps, adjust time components
    let (hour, minute, second) = if is_negative && !abs_secs.is_multiple_of(86400) {
        let remaining = abs_secs % 86400;
        let adjusted = 86400 - remaining;
        (
            ((adjusted / 3600) % 24) as u32,
            ((adjusted / 60) % 60) as u32,
            (adjusted % 60) as u32,
        )
    } else {
        (hour, minute, second)
    };

    // Days since 1970-01-01
    // Using a simplified algorithm based on Howard Hinnant's date algorithms
    let z = days + 719468; // Days from 0000-03-01 to 1970-01-01 is 719468

    let era = if z >= 0 {
        z / 146097
    } else {
        (z - 146096) / 146097
    };
    let doe = (z - era * 146097) as u32; // Day of era [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // Year of era [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // Day of year [0, 365]
    let mp = (5 * doy + 2) / 153; // Month proxy [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // Day [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // Month [1, 12]
    let y = if m <= 2 { y + 1 } else { y };

    (y as i32, m, d, hour, minute, second)
}
