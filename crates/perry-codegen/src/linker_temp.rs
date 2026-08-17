//! Failure-retention and stale-reaping policy for LLVM scratch directories.
//!
//! Kept separate from `linker.rs` so the driver stays below the repository's
//! 2,000-line source-file limit.

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

#[cfg(unix)]
use std::time::SystemTime;

use anyhow::Error;

const SCRATCH_PREFIX: &str = "perry_llvm_scratch_";
const FAILED_MARKER: &str = ".perry-failed";
const EXPLICIT_KEEP_MARKER: &str = ".perry-keep";
const STALE_AFTER: Duration = Duration::from_secs(2 * 60 * 60);

/// One ordinary failed module is enough to diagnose a compiler invocation.
/// `PERRY_LLVM_KEEP_IR` bypasses this cap because retaining every intermediate
/// is its explicit contract.
pub(super) struct FailureRetention {
    claimed: AtomicBool,
}

impl FailureRetention {
    pub(super) const fn new() -> Self {
        Self {
            claimed: AtomicBool::new(false),
        }
    }

    pub(super) fn should_retain(&self, explicit_keep: bool) -> bool {
        explicit_keep || !self.claimed.swap(true, Ordering::AcqRel)
    }
}

pub(super) static PROCESS_FAILURE_RETENTION: FailureRetention = FailureRetention::new();

/// Decide whether this failure owns the diagnostic slot, then make the error
/// agree with what remains on disk.
pub(super) struct FailedScratch<'a> {
    scratch_dir: &'a Path,
    ll_path: &'a Path,
    explicit_keep: bool,
    retention: &'a FailureRetention,
}

impl<'a> FailedScratch<'a> {
    pub(super) fn new(
        scratch_dir: &'a Path,
        ll_path: &'a Path,
        explicit_keep: bool,
        retention: &'a FailureRetention,
    ) -> Self {
        Self {
            scratch_dir,
            ll_path,
            explicit_keep,
            retention,
        }
    }

    pub(super) fn finish(&self, error: Error) -> Error {
        self.finish_with(error, None)
    }

    pub(super) fn finish_with_ir(&self, error: Error, ll_text: &str) -> Error {
        self.finish_with(error, Some(ll_text))
    }

    fn finish_with(&self, error: Error, ll_text: Option<&str>) -> Error {
        let retained = self.retention.should_retain(self.explicit_keep);
        if !retained {
            let _ = fs::remove_dir_all(self.scratch_dir);
            return error.context(
                "LLVM IR not retained: another LLVM failure in this Perry process already kept \
                 its IR (set PERRY_LLVM_KEEP_IR=1 to keep every intermediate)",
            );
        }
        if let Some(ll_text) = ll_text {
            let _ = fs::create_dir_all(self.scratch_dir);
            let _ = fs::write(self.ll_path, ll_text);
        }
        let marker = if self.explicit_keep {
            EXPLICIT_KEEP_MARKER
        } else {
            FAILED_MARKER
        };
        let _ = fs::write(self.scratch_dir.join(marker), b"");
        error.context(format!("LLVM IR left at: {}", self.ll_path.display()))
    }
}

/// Best-effort startup cleanup. A live owner always wins, even when its scratch
/// is old; explicit `PERRY_LLVM_KEEP_IR` output is never treated as stale.
pub(super) fn reap_stale_llvm_scratch_once(tmp_dir: &Path) {
    static REAPED: OnceLock<()> = OnceLock::new();
    REAPED.get_or_init(|| {
        let removed = reap_stale_llvm_scratch(tmp_dir, STALE_AFTER);
        if removed > 0 {
            log::debug!("perry-codegen: reaped {removed} stale LLVM scratch directories");
        }
    });
}

#[cfg(unix)]
pub(super) fn reap_stale_llvm_scratch(tmp_dir: &Path, min_age: Duration) -> usize {
    let Ok(entries) = fs::read_dir(tmp_dir) else {
        return 0;
    };
    let now = SystemTime::now();
    let mut removed = 0;

    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(scratch_owner_pid) else {
            continue;
        };
        let path = entry.path();
        let old_enough = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age >= min_age);
        if !old_enough || process_is_alive(pid) || was_explicitly_kept(&path) {
            continue;
        }
        if fs::remove_dir_all(&path).is_ok() {
            removed += 1;
        }
    }
    removed
}

#[cfg(not(unix))]
pub(super) fn reap_stale_llvm_scratch(_tmp_dir: &Path, _min_age: Duration) -> usize {
    // Windows has no `kill(pid, 0)` equivalent in the standard library. Keep
    // the retention cap there, but do not risk deleting another process's IR.
    0
}

#[cfg(unix)]
fn scratch_owner_pid(name: &str) -> Option<u32> {
    let suffix = name.strip_prefix(SCRATCH_PREFIX)?;
    let (pid, counter) = suffix.split_once('_')?;
    if pid.is_empty()
        || counter.is_empty()
        || counter.contains('_')
        || !counter.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return None;
    }
    let pid = u32::from_str_radix(pid, 16).ok()?;
    (pid != 0).then_some(pid)
}

#[cfg(unix)]
fn was_explicitly_kept(path: &Path) -> bool {
    if path.join(EXPLICIT_KEEP_MARKER).is_file() {
        return true;
    }
    // Successful artifacts retained before #8249 have no marker, but do have
    // compile-plan metadata. Preserve that pre-existing KEEP_IR contract.
    fs::read_dir(path).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.ends_with(".compile-plan.json"))
        })
    })
}

#[cfg(unix)]
fn process_is_alive(pid: u32) -> bool {
    use std::os::raw::c_int;

    if pid > c_int::MAX as u32 {
        return false;
    }
    extern "C" {
        fn kill(pid: c_int, signal: c_int) -> c_int;
    }
    // Signal 0 performs permission/existence checks without sending a signal.
    if unsafe { kill(pid as c_int, 0) } == 0 {
        return true;
    }
    // ESRCH is 3 on the Unix targets Perry supports. EPERM and other failures
    // mean the process may exist but is not inspectable, so preserve its files.
    std::io::Error::last_os_error().raw_os_error() != Some(3)
}
