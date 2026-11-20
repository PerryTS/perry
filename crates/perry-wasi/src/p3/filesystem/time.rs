use super::types::{ErrorCode, NewTimestamp};
use crate::p3::bindings::clocks::system_clock::Instant;
use std::time::{Duration, SystemTime};

fn magnitude(t: Instant) -> (bool, Duration) {
    // Seconds are floor-valued; nanoseconds advance even a negative second.
    // i128 also safely accommodates non-normalized u32 nanoseconds.
    let nanos = i128::from(t.seconds) * 1_000_000_000 + i128::from(t.nanoseconds);
    let magnitude = nanos.unsigned_abs();
    (
        nanos < 0,
        Duration::new(
            (magnitude / 1_000_000_000) as u64,
            (magnitude % 1_000_000_000) as u32,
        ),
    )
}
fn systemtime_from(t: Instant) -> Result<SystemTime, ErrorCode> {
    let (negative, duration) = magnitude(t);
    if negative {
        SystemTime::UNIX_EPOCH.checked_sub(duration)
    } else {
        SystemTime::UNIX_EPOCH.checked_add(duration)
    }
    .ok_or(ErrorCode::Overflow)
}
pub(super) fn systemtimespec_from(t: NewTimestamp) -> Result<Option<SystemTime>, ErrorCode> {
    match t {
        NewTimestamp::NoChange => Ok(None),
        NewTimestamp::Now => Ok(Some(SystemTime::now())),
        NewTimestamp::Timestamp(t) => systemtime_from(t).map(Some),
    }
}
impl From<crate::clocks::Datetime> for Instant {
    fn from(value: crate::clocks::Datetime) -> Self {
        Self {
            seconds: value.seconds,
            nanoseconds: value.nanoseconds,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signed_timestamp_conversion_preserves_fractional_seconds() {
        for (seconds, nanoseconds, negative, duration) in [
            (-1, 999_999_999, true, Duration::from_nanos(1)),
            (-3, 999_999_877, true, Duration::new(2, 123)),
            (-1, 1_000_000_001, false, Duration::from_nanos(1)),
            (0, 0, false, Duration::ZERO),
            (i64::MIN, 0, true, Duration::from_secs(1_u64 << 63)),
        ] {
            assert_eq!(
                magnitude(Instant {
                    seconds,
                    nanoseconds
                }),
                (negative, duration)
            );
        }
        for (seconds, nanoseconds) in [(-3, 999_999_900), (-1, 0), (0, 0), (2, 100)] {
            let time = systemtime_from(Instant {
                seconds,
                nanoseconds,
            })
            .unwrap();
            let roundtrip = crate::clocks::Datetime::try_from(time).unwrap();
            assert_eq!(
                (roundtrip.seconds, roundtrip.nanoseconds),
                (seconds, nanoseconds)
            );
        }
        assert!(matches!(
            systemtime_from(Instant {
                seconds: i64::MAX,
                nanoseconds: u32::MAX
            }),
            Err(ErrorCode::Overflow)
        ));
    }
}
