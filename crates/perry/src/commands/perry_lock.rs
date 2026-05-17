//! #498 — pinned checksums for `perry.nativeLibrary` archives.
//!
//! A swapped or tampered prebuilt static archive (`.a` / `.lib` /
//! `.dylib`) is currently undetectable: perry resolves the path and
//! hands it to the linker without inspection. A malicious dep update
//! can therefore swap in arbitrary native code and the host gets no
//! signal.
//!
//! This module closes the gap with a small SHA-256-based lockfile,
//! analogous to `package-lock.json`'s `integrity` field but scoped to
//! the linker-visible archives perry consumes through `perry.nativeLibrary.targets.<key>.prebuilt`.
//!
//! ## File shape: `<project_root>/perry.lock`
//!
//! ```json
//! {
//!   "version": 1,
//!   "native_libraries": {
//!     "@bloomengine/engine": {
//!       "macos-arm64": "sha256:abcd1234..."
//!     },
//!     "lodash": {
//!       "linux-x86_64": "sha256:..."
//!     }
//!   }
//! }
//! ```
//!
//! Per-package + per-target-arch entries so multi-target builds
//! accumulate hashes one target at a time (matching the issue's
//! "Hash covers every per-target archive declared by the package"
//! acceptance — the lockfile is the union of every target that's
//! been built).
//!
//! ## Verification semantics
//!
//! On every build:
//!
//! - Walk `ctx.native_libraries` for entries with a resolved
//!   `prebuilt: Some(path)`. Crate-source ("build from `crate_path`")
//!   entries are NOT covered by this MVP — they don't have a single
//!   file to hash. Follow-up work.
//! - For each prebuilt: compute SHA-256 of the file bytes.
//! - Look up `(package, target-arch)` in the lockfile.
//!   - **Missing entry**: write a new one (lockfile grows). Skipped in
//!     `--frozen` mode (env `PERRY_LOCK_FROZEN=1`) where missing
//!     entries fail the build — useful for CI verification.
//!   - **Matching entry**: pass through.
//!   - **Mismatching entry**: fail the build with an actionable
//!     diagnostic that names the package and tells the user how to
//!     deliberately bump.
//!
//! ## Updating after a deliberate dep change
//!
//! - **Refresh**: set `PERRY_LOCK_REFRESH=1` for one build. Any
//!   mismatching entry is rewritten with the current hash instead of
//!   failing. The replacement is verbose so it's obvious in the
//!   build log.
//! - **Wipe**: delete `perry.lock` and rebuild. The fresh build
//!   writes a new lockfile from scratch (same code path as first-time
//!   resolution).
//!
//! ## What this does NOT cover (deferred)
//!
//! - Crate-source builds (`crate_path` field) — multi-file builds
//!   need a different hash mechanism (Cargo.lock + workspace hash).
//! - `perry lock --update <pkg>` CLI subcommand — env-var refresh is
//!   sufficient for an MVP.
//! - Cross-platform pre-hashing (hashing all targets at once instead
//!   of accumulating per build).
//! - `perry lock --frozen` CLI flag — `PERRY_LOCK_FROZEN=1` env var
//!   covers the CI verification-only use case.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Lockfile filename relative to the project root. Named to mirror
/// `package-lock.json` / `Cargo.lock` so reviewers recognise the
/// intent immediately.
pub const LOCKFILE_NAME: &str = "perry.lock";

/// Top-level lockfile shape. Versioned so future entry kinds (e.g.
/// crate-source hashes, `compilePackages` source-tree hashes) can
/// land without breaking existing lockfiles.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PerryLock {
    pub version: u32,
    /// `package_name` → `target_arch_key` → `sha256:hex`. BTreeMap
    /// for both levels so the on-disk JSON is byte-deterministic
    /// across builds.
    #[serde(default)]
    pub native_libraries: BTreeMap<String, BTreeMap<String, String>>,
}

impl PerryLock {
    pub fn new() -> Self {
        Self {
            version: 1,
            native_libraries: BTreeMap::new(),
        }
    }
}

/// Read `<project_root>/perry.lock` if present. A missing file is
/// not an error — first-time builds start from an empty lockfile.
pub fn read_lock(project_root: &Path) -> Result<PerryLock> {
    let path = project_root.join(LOCKFILE_NAME);
    if !path.exists() {
        return Ok(PerryLock::new());
    }
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let lock: PerryLock = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(lock)
}

/// Write `perry.lock` to the project root with stable JSON encoding.
pub fn write_lock(project_root: &Path, lock: &PerryLock) -> Result<()> {
    let path = project_root.join(LOCKFILE_NAME);
    let json = serde_json::to_string_pretty(lock)
        .with_context(|| format!("failed to serialize {}", path.display()))?;
    std::fs::write(&path, json).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

/// SHA-256 of a file as `"sha256:<hex>"`. Streaming hash so very
/// large prebuilt archives don't OOM. Mirrors the existing
/// `commands::updater::sha256_file` helper but returns the
/// canonical-prefixed form so the lockfile is self-describing
/// (room to add `sha512:` etc. later without a version bump).
pub fn sha256_of_file(path: &Path) -> Result<String> {
    use std::io::Read;
    let mut file =
        std::fs::File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest = hasher.finalize();
    Ok(format!("sha256:{:x}", digest))
}

/// One archive-resolution that needs hashing. The driver assembles
/// these from `ctx.native_libraries` and passes the list to
/// [`verify_and_update`].
#[derive(Debug, Clone)]
pub struct ArchiveEntry {
    /// `perry.nativeLibrary` package name (`module` field in the
    /// `NativeLibraryManifest`).
    pub package: String,
    /// Per-target-arch key — same shape `targets.<key>` uses in
    /// `package.json` (e.g. `macos-arm64`, `linux-x86_64`). When
    /// the manifest only had a bare `targets.macos` entry, the
    /// key is `"macos"` (no architecture suffix).
    pub target_arch: String,
    /// Absolute path to the resolved prebuilt archive on disk.
    pub archive_path: PathBuf,
}

/// Outcome of the verify-and-update pass — caller decides what to
/// do with it (the compile driver writes the lockfile back and
/// emits a status line in Text mode).
#[derive(Debug, Default)]
pub struct LockOutcome {
    /// New `(package, target_arch)` entries that were absent before
    /// this build and got added.
    pub added: Vec<(String, String)>,
    /// Entries that existed and verified against the same hash.
    pub verified: Vec<(String, String)>,
    /// Entries that existed but whose hash now differs and were
    /// rewritten because `PERRY_LOCK_REFRESH=1` was set.
    pub refreshed: Vec<(String, String)>,
    /// Did the lockfile change vs what was read from disk? Used by
    /// the driver to decide whether to write back.
    pub modified: bool,
}

/// Cross-check `entries` against `lock`. Adds new entries, verifies
/// matching ones, fails on mismatches. Honors the two env-var knobs:
///
/// - `PERRY_LOCK_REFRESH=1` — rewrite mismatched entries instead of
///   failing.
/// - `PERRY_LOCK_FROZEN=1` — refuse to add missing entries. Both a
///   missing entry and a mismatch fail the build under this mode.
pub fn verify_and_update(lock: &mut PerryLock, entries: &[ArchiveEntry]) -> Result<LockOutcome> {
    let refresh = std::env::var("PERRY_LOCK_REFRESH")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let frozen = std::env::var("PERRY_LOCK_FROZEN")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    let mut outcome = LockOutcome::default();
    for entry in entries {
        let actual = sha256_of_file(&entry.archive_path).with_context(|| {
            format!(
                "failed to hash native-library archive `{}` for `{}`",
                entry.archive_path.display(),
                entry.package
            )
        })?;
        let pkg_bucket = lock
            .native_libraries
            .entry(entry.package.clone())
            .or_default();
        match pkg_bucket.get(&entry.target_arch) {
            Some(locked) if locked == &actual => {
                outcome
                    .verified
                    .push((entry.package.clone(), entry.target_arch.clone()));
            }
            Some(locked) => {
                if refresh {
                    pkg_bucket.insert(entry.target_arch.clone(), actual.clone());
                    outcome
                        .refreshed
                        .push((entry.package.clone(), entry.target_arch.clone()));
                    outcome.modified = true;
                } else {
                    bail!(
                        "archive for `{pkg}` (target `{tgt}`) changed since last accepted:\n\
                         \n\
                           expected: {locked}\n\
                           found:    {actual}\n\
                           path:     {path}\n\
                         \n\
                         Review the package — a swapped or tampered prebuilt static\n\
                         archive is exactly the supply-chain attack class this lock\n\
                         was added to catch (#498).\n\
                         \n\
                         If the change is intentional (dep upgrade, vendored archive\n\
                         rebuild), rerun the build with `PERRY_LOCK_REFRESH=1` to\n\
                         rewrite the lockfile entry, or delete `perry.lock` to\n\
                         regenerate it from scratch.",
                        pkg = entry.package,
                        tgt = entry.target_arch,
                        locked = locked,
                        actual = actual,
                        path = entry.archive_path.display(),
                    );
                }
            }
            None => {
                if frozen {
                    bail!(
                        "no `perry.lock` entry for `{pkg}` (target `{tgt}`):\n\
                         \n\
                           archive: {path}\n\
                           hash:    {actual}\n\
                         \n\
                         `PERRY_LOCK_FROZEN=1` is set — refusing to silently add new\n\
                         lockfile entries. This is the CI-friendly verification mode.\n\
                         \n\
                         If the package is new to this build (dep added, target\n\
                         expanded), run a local build without `PERRY_LOCK_FROZEN=1`\n\
                         to populate the lockfile, then commit the updated\n\
                         `perry.lock`.",
                        pkg = entry.package,
                        tgt = entry.target_arch,
                        path = entry.archive_path.display(),
                        actual = actual,
                    );
                }
                pkg_bucket.insert(entry.target_arch.clone(), actual);
                outcome
                    .added
                    .push((entry.package.clone(), entry.target_arch.clone()));
                outcome.modified = true;
            }
        }
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_archive(dir: &Path, name: &str, body: &[u8]) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn round_trip_empty_lock() {
        let tmp = tempfile::tempdir().unwrap();
        let lock = PerryLock::new();
        write_lock(tmp.path(), &lock).unwrap();
        let read = read_lock(tmp.path()).unwrap();
        assert_eq!(read, lock);
    }

    #[test]
    fn missing_file_reads_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let lock = read_lock(tmp.path()).unwrap();
        assert!(lock.native_libraries.is_empty());
        assert_eq!(lock.version, 1);
    }

    #[test]
    fn sha256_of_file_matches_known_vector() {
        let tmp = tempfile::tempdir().unwrap();
        let path = write_archive(tmp.path(), "x.a", b"hello\n");
        // sha256 of "hello\n" (6 bytes).
        let hash = sha256_of_file(&path).unwrap();
        assert_eq!(
            hash,
            "sha256:5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03"
        );
    }

    #[test]
    fn first_build_adds_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = write_archive(tmp.path(), "lib.a", b"v1");
        let mut lock = PerryLock::new();
        std::env::remove_var("PERRY_LOCK_REFRESH");
        std::env::remove_var("PERRY_LOCK_FROZEN");
        let outcome = verify_and_update(
            &mut lock,
            &[ArchiveEntry {
                package: "demo".into(),
                target_arch: "macos-arm64".into(),
                archive_path: archive,
            }],
        )
        .unwrap();
        assert_eq!(outcome.added.len(), 1);
        assert!(outcome.modified);
        assert!(lock.native_libraries.contains_key("demo"));
    }

    #[test]
    fn matching_hash_verifies() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = write_archive(tmp.path(), "lib.a", b"v1");
        let mut lock = PerryLock::new();
        std::env::remove_var("PERRY_LOCK_REFRESH");
        std::env::remove_var("PERRY_LOCK_FROZEN");
        // Seed the lock with the known-good hash.
        let h = sha256_of_file(&archive).unwrap();
        lock.native_libraries
            .entry("demo".into())
            .or_default()
            .insert("macos-arm64".into(), h.clone());
        let outcome = verify_and_update(
            &mut lock,
            &[ArchiveEntry {
                package: "demo".into(),
                target_arch: "macos-arm64".into(),
                archive_path: archive,
            }],
        )
        .unwrap();
        assert_eq!(outcome.verified.len(), 1);
        assert!(!outcome.modified);
    }

    #[test]
    fn mismatching_hash_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = write_archive(tmp.path(), "lib.a", b"v2-tampered");
        let mut lock = PerryLock::new();
        std::env::remove_var("PERRY_LOCK_REFRESH");
        std::env::remove_var("PERRY_LOCK_FROZEN");
        lock.native_libraries
            .entry("demo".into())
            .or_default()
            .insert("macos-arm64".into(), "sha256:stale".into());
        let err = verify_and_update(
            &mut lock,
            &[ArchiveEntry {
                package: "demo".into(),
                target_arch: "macos-arm64".into(),
                archive_path: archive,
            }],
        )
        .expect_err("mismatch must fail");
        let msg = err.to_string();
        assert!(msg.contains("changed since last accepted"), "{msg}");
        assert!(msg.contains("PERRY_LOCK_REFRESH=1"), "{msg}");
        assert!(msg.contains("(#498)"), "{msg}");
    }

    #[test]
    fn refresh_env_var_rewrites_mismatch() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = write_archive(tmp.path(), "lib.a", b"v2-new");
        let mut lock = PerryLock::new();
        lock.native_libraries
            .entry("demo".into())
            .or_default()
            .insert("macos-arm64".into(), "sha256:stale".into());
        std::env::set_var("PERRY_LOCK_REFRESH", "1");
        std::env::remove_var("PERRY_LOCK_FROZEN");
        let outcome = verify_and_update(
            &mut lock,
            &[ArchiveEntry {
                package: "demo".into(),
                target_arch: "macos-arm64".into(),
                archive_path: archive.clone(),
            }],
        )
        .unwrap();
        std::env::remove_var("PERRY_LOCK_REFRESH");
        assert_eq!(outcome.refreshed.len(), 1);
        assert!(outcome.modified);
        // Entry now matches the current archive.
        let new_hash = sha256_of_file(&archive).unwrap();
        assert_eq!(lock.native_libraries["demo"]["macos-arm64"], new_hash);
    }

    #[test]
    fn frozen_env_var_refuses_new_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = write_archive(tmp.path(), "lib.a", b"v1");
        let mut lock = PerryLock::new();
        std::env::set_var("PERRY_LOCK_FROZEN", "1");
        std::env::remove_var("PERRY_LOCK_REFRESH");
        let err = verify_and_update(
            &mut lock,
            &[ArchiveEntry {
                package: "demo".into(),
                target_arch: "macos-arm64".into(),
                archive_path: archive,
            }],
        )
        .expect_err("frozen mode must reject new entries");
        std::env::remove_var("PERRY_LOCK_FROZEN");
        let msg = err.to_string();
        assert!(msg.contains("`PERRY_LOCK_FROZEN=1`"), "{msg}");
        assert!(msg.contains("CI"), "{msg}");
    }

    #[test]
    fn lockfile_is_deterministic_across_writes() {
        // BTreeMap on both levels means the on-disk JSON has stable
        // key order — important for `git diff` to be meaningful.
        let tmp = tempfile::tempdir().unwrap();
        let mut lock = PerryLock::new();
        lock.native_libraries
            .entry("b-pkg".into())
            .or_default()
            .insert("linux-x86_64".into(), "sha256:bbb".into());
        lock.native_libraries
            .entry("a-pkg".into())
            .or_default()
            .insert("macos-arm64".into(), "sha256:aaa".into());
        write_lock(tmp.path(), &lock).unwrap();
        let raw = std::fs::read_to_string(tmp.path().join(LOCKFILE_NAME)).unwrap();
        // `a-pkg` (sorted first) appears before `b-pkg`.
        let a_pos = raw.find("a-pkg").unwrap();
        let b_pos = raw.find("b-pkg").unwrap();
        assert!(a_pos < b_pos, "BTreeMap order broken: {raw}");
    }
}
