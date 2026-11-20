// Adapted from Wasmtime 48.0.5; Apache-2.0 WITH LLVM-exception. See PROVENANCE.md.
#![allow(clippy::useless_conversion)]

use crate::filesystem::primitives::{ImplFileTypeExt, Metadata};
#[cfg(target_os = "linux")]
use rustix::fs::{makedev, Statx, StatxFlags};
use rustix::fs::{RawMode, Stat};
use std::time::{Duration, SystemTime};
use std::{fs, io};

#[derive(Debug, Clone)]
pub(crate) struct ImplMetadataExt {
    dev: u64,
    ino: u64,
    nlink: u64,
}

impl ImplMetadataExt {
    /// Constructs a new instance of `Self` from the given [`std::fs::File`]
    /// and [`std::fs::Metadata`].
    #[inline]
    #[allow(clippy::unnecessary_wraps)]
    pub(crate) fn from(_file: &fs::File, std: &fs::Metadata) -> io::Result<Self> {
        // On `rustix`-style platforms, the `Metadata` has everything we need.
        Ok(Self::from_just_metadata(std))
    }

    /// Constructs a new instance of `Self` from the given
    /// [`std::fs::Metadata`].
    #[inline]
    pub(crate) fn from_just_metadata(std: &fs::Metadata) -> Self {
        use rustix::fs::MetadataExt;
        Self {
            dev: std.dev(),
            ino: std.ino(),
            nlink: std.nlink(),
        }
    }

    /// Constructs a new instance of `Metadata` from the given `Stat`.
    #[inline]
    #[allow(unused_comparisons)] // NB: rust-lang/rust#115823 requires this here instead of on `st_dev` processing below
    pub(crate) fn from_rustix(stat: Stat) -> Metadata {
        Metadata {
            file_type: ImplFileTypeExt::from_raw_mode(stat.st_mode as RawMode),
            len: u64::try_from(stat.st_size).unwrap(),

            #[cfg(not(target_os = "wasi"))]
            modified: system_time_from_rustix(
                stat.st_mtime.try_into().unwrap(),
                stat.st_mtime_nsec as _,
            ),
            #[cfg(not(target_os = "wasi"))]
            accessed: system_time_from_rustix(
                stat.st_atime.try_into().unwrap(),
                stat.st_atime_nsec as _,
            ),

            #[cfg(target_os = "wasi")]
            modified: system_time_from_rustix(stat.st_mtim.tv_sec, stat.st_mtim.tv_nsec as _),
            #[cfg(target_os = "wasi")]
            accessed: system_time_from_rustix(stat.st_atim.tv_sec, stat.st_atim.tv_nsec as _),

            #[cfg(any(
                target_os = "freebsd",
                target_os = "openbsd",
                target_os = "netbsd",
                target_os = "macos",
                target_os = "ios",
                target_os = "tvos",
                target_os = "watchos",
                target_os = "visionos",
            ))]
            created: system_time_from_rustix(
                stat.st_birthtime.try_into().unwrap(),
                stat.st_birthtime_nsec as _,
            ),

            // `stat.st_ctime` is the latest status change; we want the creation.
            #[cfg(not(any(
                target_os = "freebsd",
                target_os = "openbsd",
                target_os = "macos",
                target_os = "ios",
                target_os = "tvos",
                target_os = "watchos",
                target_os = "visionos",
                target_os = "netbsd"
            )))]
            created: None,

            ext: Self {
                // dev_t is signed on some platforms. Integer casting preserves
                // the upstream sign extension for negative device identifiers.
                dev: stat.st_dev as _,
                ino: stat.st_ino.into(),
                nlink: u64::from(stat.st_nlink),
            },
        }
    }

    /// Constructs a new instance of `Metadata` from the given `Statx`.
    #[cfg(target_os = "linux")]
    #[inline]
    pub(crate) fn from_rustix_statx(statx: Statx) -> Metadata {
        Metadata {
            file_type: ImplFileTypeExt::from_raw_mode(RawMode::from(statx.stx_mode)),
            len: u64::try_from(statx.stx_size).unwrap(),
            modified: if statx.stx_mask & StatxFlags::MTIME.bits() != 0 {
                system_time_from_rustix(statx.stx_mtime.tv_sec, statx.stx_mtime.tv_nsec as _)
            } else {
                None
            },
            accessed: if statx.stx_mask & StatxFlags::ATIME.bits() != 0 {
                system_time_from_rustix(statx.stx_atime.tv_sec, statx.stx_atime.tv_nsec as _)
            } else {
                None
            },
            created: if statx.stx_mask & StatxFlags::BTIME.bits() != 0 {
                system_time_from_rustix(statx.stx_btime.tv_sec, statx.stx_btime.tv_nsec as _)
            } else {
                None
            },

            ext: Self {
                dev: makedev(statx.stx_dev_major, statx.stx_dev_minor),
                ino: statx.stx_ino.into(),
                nlink: u64::from(statx.stx_nlink),
            },
        }
    }
}

#[allow(clippy::similar_names)]
fn system_time_from_rustix(sec: i64, nsec: u64) -> Option<SystemTime> {
    if sec >= 0 {
        SystemTime::UNIX_EPOCH.checked_add(Duration::new(u64::try_from(sec).unwrap(), nsec as _))
    } else {
        SystemTime::UNIX_EPOCH
            .checked_sub(Duration::new(sec.unsigned_abs(), 0))
            .and_then(|t| t.checked_add(Duration::new(0, nsec as u32)))
    }
}

impl crate::filesystem::primitives::MetadataExt for ImplMetadataExt {
    #[inline]
    fn dev(&self) -> u64 {
        self.dev
    }

    #[inline]
    fn ino(&self) -> u64 {
        self.ino
    }

    #[inline]
    fn nlink(&self) -> u64 {
        self.nlink
    }
}

/// It should be possible to represent times before the Epoch.
/// https://github.com/bytecodealliance/cap-std/issues/328
#[test]
fn negative_time() {
    let system_time = system_time_from_rustix(-1, 1).unwrap();
    let d = SystemTime::UNIX_EPOCH.duration_since(system_time).unwrap();
    assert_eq!(d.as_secs(), 0);
    assert_eq!(d.subsec_nanos(), 999999999);
}
