// Adapted from Wasmtime 48.0.5; Apache-2.0 WITH LLVM-exception. See PROVENANCE.md.
use std::{
    error::Error,
    fmt,
    time::{Duration, SystemTime},
};
pub(crate) struct Datetime {
    pub seconds: i64,
    pub nanoseconds: u32,
}

impl TryFrom<SystemTime> for Datetime {
    type Error = DatetimeError;

    fn try_from(time: SystemTime) -> Result<Self, Self::Error> {
        let epoch = SystemTime::UNIX_EPOCH;

        if time >= epoch {
            let duration = time.duration_since(epoch)?;
            Ok(Self {
                seconds: duration.as_secs().try_into()?,
                nanoseconds: duration.subsec_nanos(),
            })
        } else {
            Self::before_epoch(epoch.duration_since(time)?)
        }
    }
}

impl Datetime {
    fn before_epoch(duration: Duration) -> Result<Self, DatetimeError> {
        let nanos = duration.subsec_nanos();
        Ok(Self {
            // Signed seconds are the floor; positive nanoseconds advance
            // toward the epoch even for negative timestamps.
            seconds: (-i128::from(duration.as_secs()) - i128::from(nanos != 0)).try_into()?,
            nanoseconds: if nanos == 0 { 0 } else { 1_000_000_000 - nanos },
        })
    }
}

#[derive(Debug)]
pub struct DatetimeError;

impl fmt::Display for DatetimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("couldn't represent time as a WASI `Datetime`")
    }
}

impl Error for DatetimeError {}

impl From<std::time::SystemTimeError> for DatetimeError {
    fn from(_: std::time::SystemTimeError) -> Self {
        DatetimeError
    }
}

impl From<std::num::TryFromIntError> for DatetimeError {
    fn from(_: std::num::TryFromIntError) -> Self {
        DatetimeError
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn fractional_pre_epoch_times_use_floor_seconds() {
        for (duration, expected) in [
            (Duration::from_nanos(1), (-1, 999_999_999)),
            (Duration::from_secs(1), (-1, 0)),
            (Duration::new(2, 123), (-3, 999_999_877)),
        ] {
            // Windows SystemTime has 100 ns precision; test the full WASI
            // arithmetic independently of the host timestamp representation.
            let value = Datetime::before_epoch(duration).unwrap();
            assert_eq!((value.seconds, value.nanoseconds), expected);
            assert!(crate::p2::bindings::clocks::wall_clock::Datetime::try_from(value).is_err());
        }
        let epoch = Datetime::try_from(SystemTime::UNIX_EPOCH).unwrap();
        assert_eq!((epoch.seconds, epoch.nanoseconds), (0, 0));
        let after = Datetime::try_from(SystemTime::UNIX_EPOCH + Duration::new(2, 100)).unwrap();
        assert_eq!((after.seconds, after.nanoseconds), (2, 100));
        let before = Datetime::try_from(SystemTime::UNIX_EPOCH - Duration::new(2, 100)).unwrap();
        assert_eq!((before.seconds, before.nanoseconds), (-3, 999_999_900));
    }
}
