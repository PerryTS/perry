//! UTC formatting for CLI metadata; no local-time-zone database.
use std::time::{SystemTime, UNIX_EPOCH};

fn now_seconds() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64) - i64::from(e.duration().subsec_nanos() != 0),
    }
}
// Proleptic Gregorian civil date from days since 1970-01-01. Split into
// 400-year eras so leap centuries and dates before the epoch work alike.
fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    (y + i64::from(m <= 2), m, d)
}
pub fn current_year() -> i64 {
    civil(now_seconds().div_euclid(86400)).0
}
pub fn now_rfc3339() -> String {
    format_rfc3339(now_seconds())
}
fn format_rfc3339(seconds: i64) -> String {
    let (y, m, d) = civil(seconds.div_euclid(86400));
    let time = seconds.rem_euclid(86400);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}+00:00",
        time / 3600,
        time / 60 % 60,
        time % 60
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gregorian_boundaries() {
        assert_eq!(format_rfc3339(0), "1970-01-01T00:00:00+00:00");
        assert_eq!(format_rfc3339(-1), "1969-12-31T23:59:59+00:00");
        assert_eq!(format_rfc3339(951782400), "2000-02-29T00:00:00+00:00");
        assert_eq!(format_rfc3339(4107542400), "2100-03-01T00:00:00+00:00");
    }
}
