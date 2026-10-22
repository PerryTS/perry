// Adapted from Wasmtime 48.0.5; Apache-2.0 WITH LLVM-exception. See PROVENANCE.md.
use std::{error::Error, fmt, time::SystemTime};
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
            let duration = epoch.duration_since(time)?;
            Ok(Self {
                seconds: -duration.as_secs().try_into()?,
                nanoseconds: duration.subsec_nanos(),
            })
        }
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
