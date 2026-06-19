//! #498 — pinned SHA-256 checksums for `perry.nativeLibrary` archives.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const LOCK_FILENAME: &str = "perry.lock";
pub const LOCK_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PerryLock {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub native_library: BTreeMap<String, NativeLibraryLock>,
}

fn default_version() -> u32 {
    LOCK_VERSION
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct NativeLibraryLock {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sha256_per_target: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum LockMode {
    #[default]
    Default,
    Update(Vec<String>),
    Frozen,
}

impl LockMode {
    pub fn allows_writes(&self) -> bool {
        !matches!(self, LockMode::Frozen)
    }
    pub fn allows_refresh(&self, package: &str) -> bool {
        match self {
            LockMode::Update(pkgs) => pkgs.is_empty() || pkgs.iter().any(|p| p == package),
            _ => false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ArchiveEntry {
    pub package: String,
    pub target_key: String,
    pub path: PathBuf,
}

pub fn verify_or_write(
    project_root: &Path,
    archives: &[ArchiveEntry],
    mode: &LockMode,
) -> Result<PerryLock> {
    let lock_path = project_root.join(LOCK_FILENAME);
    let mut lock = load_or_default(&lock_path)?;
    let mut mismatches: Vec<MismatchReport> = Vec::new();
    let mut missing_frozen: Vec<MissingReport> = Vec::new();
    let mut dirty = false;

    for archive in archives {
        let actual = sha256_of_file(&archive.path)?;
        let entry = lock
            .native_library
            .entry(archive.package.clone())
            .or_default();
        match entry.sha256_per_target.get(&archive.target_key) {
            Some(expected) if expected == &actual => {}
            Some(expected) => {
                if mode.allows_refresh(&archive.package) {
                    entry
                        .sha256_per_target
                        .insert(archive.target_key.clone(), actual);
                    dirty = true;
                } else {
                    mismatches.push(MismatchReport {
                        package: archive.package.clone(),
                        target_key: archive.target_key.clone(),
                        path: archive.path.clone(),
                        expected: expected.clone(),
                        actual,
                    });
                }
            }
            None => {
                if matches!(mode, LockMode::Frozen) {
                    missing_frozen.push(MissingReport {
                        package: archive.package.clone(),
                        target_key: archive.target_key.clone(),
                        path: archive.path.clone(),
                        actual,
                    });
                } else if mode.allows_writes() {
                    entry
                        .sha256_per_target
                        .insert(archive.target_key.clone(), actual);
                    dirty = true;
                }
            }
        }
    }

    if !mismatches.is_empty() || !missing_frozen.is_empty() {
        return Err(anyhow!(format_violation(
            &mismatches,
            &missing_frozen,
            mode
        )));
    }
    if dirty && mode.allows_writes() {
        save_lock(&lock_path, &lock)?;
    }
    Ok(lock)
}

pub fn sha256_of_file(path: &Path) -> Result<String> {
    use std::io::Read;
    let mut file = fs::File::open(path)
        .map_err(|e| anyhow!("open archive {} for hashing: {}", path.display(), e))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| anyhow!("read archive {} for hashing: {}", path.display(), e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

pub fn load_or_default(lock_path: &Path) -> Result<PerryLock> {
    if !lock_path.exists() {
        return Ok(PerryLock {
            version: LOCK_VERSION,
            native_library: BTreeMap::new(),
        });
    }
    let body = fs::read_to_string(lock_path)
        .map_err(|e| anyhow!("read {}: {}", lock_path.display(), e))?;
    let parsed: PerryLock = toml::from_str(&body).map_err(|e| {
        anyhow!("parse {}: {}\n\nThis file should be committed alongside your source; do not edit it by hand. Delete it to regenerate.", lock_path.display(), e)
    })?;
    Ok(parsed)
}

pub fn save_lock(lock_path: &Path, lock: &PerryLock) -> Result<()> {
    let body = toml::to_string_pretty(lock)
        .map_err(|e| anyhow!("serialize {}: {}", lock_path.display(), e))?;
    let tmp = lock_path.with_extension("lock.tmp");
    fs::write(&tmp, body.as_bytes()).map_err(|e| anyhow!("write {}: {}", tmp.display(), e))?;
    fs::rename(&tmp, lock_path)
        .map_err(|e| anyhow!("rename {} -> {}: {}", tmp.display(), lock_path.display(), e))?;
    Ok(())
}

#[derive(Debug)]
struct MismatchReport {
    package: String,
    target_key: String,
    path: PathBuf,
    expected: String,
    actual: String,
}

#[derive(Debug)]
struct MissingReport {
    package: String,
    target_key: String,
    path: PathBuf,
    actual: String,
}

fn format_violation(
    mismatches: &[MismatchReport],
    missing_frozen: &[MissingReport],
    mode: &LockMode,
) -> String {
    let mut out = String::new();
    if !mismatches.is_empty() {
        out.push_str(
            "Lockfile mismatch: one or more archives changed since they were locked (#498).\n\n",
        );
        for m in mismatches {
            out.push_str(&format!(
                "  archive for `{}` (target `{}`) changed since last accepted:\n    expected: sha256:{}\n    found:    sha256:{}\n    path:     {}\n\n",
                m.package, m.target_key, m.expected, m.actual, m.path.display(),
            ));
        }
        let mut names: Vec<&str> = mismatches.iter().map(|m| m.package.as_str()).collect();
        names.sort();
        names.dedup();
        out.push_str("Review the changes - a swapped or tampered prebuilt archive is exactly\n");
        out.push_str("the supply-chain attack class this lock was added to catch.\n\n");
        out.push_str("If the change is intentional (dep upgrade, vendored rebuild), run:\n\n");
        for name in &names {
            out.push_str(&format!("    perry lock --update {}\n", name));
        }
        out.push('\n');
    }
    if !missing_frozen.is_empty() {
        if matches!(mode, LockMode::Frozen) {
            out.push_str(
                "Frozen-mode lockfile is missing entries that the current build needs (#498).\n",
            );
            out.push_str(
                "`perry lock --frozen` refuses to write to perry.lock; commit the updated\n",
            );
            out.push_str("lockfile from a non-frozen build instead.\n\n");
        }
        for m in missing_frozen {
            out.push_str(&format!(
                "  no lock entry for `{}` (target `{}`):\n    found:    sha256:{}\n    path:     {}\n\n",
                m.package, m.target_key, m.actual, m.path.display(),
            ));
        }
        out.push_str("To create / refresh the lockfile, run a regular build (or `perry lock`)\n");
        out.push_str("without `--frozen`, then commit `perry.lock`.\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    fn write_archive(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.join(name);
        let mut f = fs::File::create(&path).expect("create archive fixture");
        f.write_all(bytes).expect("write archive fixture");
        path
    }

    #[test]
    fn sha256_of_file_matches_known_vector() {
        let dir = tempdir().unwrap();
        let p = write_archive(dir.path(), "hello.bin", b"hello\n");
        let hash = sha256_of_file(&p).expect("hash hello.bin");
        assert_eq!(
            hash,
            "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03"
        );
    }

    #[test]
    fn round_trip_empty_lock() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("perry.lock");
        let lock = PerryLock::default();
        save_lock(&path, &lock).expect("save");
        let round_tripped = load_or_default(&path).expect("load");
        assert_eq!(round_tripped, lock);
    }

    #[test]
    fn missing_file_reads_empty() {
        let dir = tempdir().unwrap();
        let lock = load_or_default(&dir.path().join("perry.lock")).expect("load missing");
        assert!(lock.native_library.is_empty());
        assert_eq!(lock.version, LOCK_VERSION);
    }

    #[test]
    fn first_build_adds_entries() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        let a = write_archive(project, "a.a", b"alpha");
        let b = write_archive(project, "b.a", b"beta");
        let archives = vec![
            ArchiveEntry {
                package: "pkg-a".into(),
                target_key: "macos-arm64".into(),
                path: a.clone(),
            },
            ArchiveEntry {
                package: "pkg-b".into(),
                target_key: "linux-x86_64".into(),
                path: b.clone(),
            },
        ];
        let lock = verify_or_write(project, &archives, &LockMode::Default).expect("write");
        assert_eq!(
            lock.native_library["pkg-a"].sha256_per_target["macos-arm64"],
            sha256_of_file(&a).unwrap()
        );
        assert_eq!(
            lock.native_library["pkg-b"].sha256_per_target["linux-x86_64"],
            sha256_of_file(&b).unwrap()
        );
        assert!(project.join("perry.lock").exists());
    }

    #[test]
    fn matching_hash_verifies() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        let a = write_archive(project, "a.a", b"alpha");
        let archives = vec![ArchiveEntry {
            package: "pkg-a".into(),
            target_key: "macos-arm64".into(),
            path: a,
        }];
        verify_or_write(project, &archives, &LockMode::Default).expect("first write");
        verify_or_write(project, &archives, &LockMode::Default).expect("verify pass");
    }

    #[test]
    fn mismatching_hash_fails() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        let a = write_archive(project, "a.a", b"alpha");
        verify_or_write(
            project,
            &[ArchiveEntry {
                package: "pkg-a".into(),
                target_key: "macos-arm64".into(),
                path: a.clone(),
            }],
            &LockMode::Default,
        )
        .expect("initial");
        write_archive(project, "a.a", b"ALPHA-MUTATED");
        let err = verify_or_write(
            project,
            &[ArchiveEntry {
                package: "pkg-a".into(),
                target_key: "macos-arm64".into(),
                path: a,
            }],
            &LockMode::Default,
        )
        .expect_err("mismatch fails");
        let msg = err.to_string();
        assert!(msg.contains("pkg-a"), "names package: {msg}");
        assert!(
            msg.contains("perry lock --update pkg-a"),
            "suggests update: {msg}"
        );
        assert!(msg.contains("macos-arm64"), "names target: {msg}");
        assert!(msg.contains("#498"), "cites issue: {msg}");
    }

    #[test]
    fn update_mode_rewrites_mismatch() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        let a = write_archive(project, "a.a", b"alpha");
        verify_or_write(
            project,
            &[ArchiveEntry {
                package: "pkg-a".into(),
                target_key: "macos-arm64".into(),
                path: a.clone(),
            }],
            &LockMode::Default,
        )
        .expect("seed");
        write_archive(project, "a.a", b"ALPHA-V2");
        let lock = verify_or_write(
            project,
            &[ArchiveEntry {
                package: "pkg-a".into(),
                target_key: "macos-arm64".into(),
                path: a.clone(),
            }],
            &LockMode::Update(vec!["pkg-a".into()]),
        )
        .expect("refresh");
        assert_eq!(
            lock.native_library["pkg-a"].sha256_per_target["macos-arm64"],
            sha256_of_file(&a).unwrap()
        );
    }

    #[test]
    fn update_mode_empty_refreshes_all() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        let a = write_archive(project, "a.a", b"alpha");
        let b = write_archive(project, "b.a", b"beta");
        let archives_v1 = vec![
            ArchiveEntry {
                package: "pkg-a".into(),
                target_key: "macos-arm64".into(),
                path: a.clone(),
            },
            ArchiveEntry {
                package: "pkg-b".into(),
                target_key: "linux-x86_64".into(),
                path: b.clone(),
            },
        ];
        verify_or_write(project, &archives_v1, &LockMode::Default).expect("seed");
        write_archive(project, "a.a", b"alpha-v2");
        write_archive(project, "b.a", b"beta-v2");
        let lock =
            verify_or_write(project, &archives_v1, &LockMode::Update(vec![])).expect("refresh-all");
        assert_eq!(
            lock.native_library["pkg-a"].sha256_per_target["macos-arm64"],
            sha256_of_file(&a).unwrap()
        );
        assert_eq!(
            lock.native_library["pkg-b"].sha256_per_target["linux-x86_64"],
            sha256_of_file(&b).unwrap()
        );
    }

    #[test]
    fn update_mode_only_refreshes_named_packages() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        let a = write_archive(project, "a.a", b"alpha");
        let b = write_archive(project, "b.a", b"beta");
        let archives_v1 = vec![
            ArchiveEntry {
                package: "pkg-a".into(),
                target_key: "macos-arm64".into(),
                path: a.clone(),
            },
            ArchiveEntry {
                package: "pkg-b".into(),
                target_key: "linux-x86_64".into(),
                path: b.clone(),
            },
        ];
        verify_or_write(project, &archives_v1, &LockMode::Default).expect("seed");
        write_archive(project, "a.a", b"alpha-v2");
        write_archive(project, "b.a", b"beta-v2");
        let err = verify_or_write(
            project,
            &archives_v1,
            &LockMode::Update(vec!["pkg-a".into()]),
        )
        .expect_err("pkg-b not refreshed");
        assert!(err.to_string().contains("pkg-b"));
    }

    #[test]
    fn frozen_mode_refuses_new_entries() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        let a = write_archive(project, "a.a", b"alpha");
        let archives = vec![ArchiveEntry {
            package: "pkg-a".into(),
            target_key: "macos-arm64".into(),
            path: a,
        }];
        let err = verify_or_write(project, &archives, &LockMode::Frozen)
            .expect_err("frozen+missing fails");
        let msg = err.to_string();
        assert!(
            msg.contains("Frozen") || msg.contains("frozen"),
            "mentions frozen: {msg}"
        );
        assert!(msg.contains("pkg-a"));
        assert!(
            !project.join("perry.lock").exists(),
            "frozen must not write"
        );
    }

    #[test]
    fn frozen_mode_passes_when_lock_matches() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        let a = write_archive(project, "a.a", b"alpha");
        let archives = vec![ArchiveEntry {
            package: "pkg-a".into(),
            target_key: "macos-arm64".into(),
            path: a,
        }];
        verify_or_write(project, &archives, &LockMode::Default).expect("seed");
        verify_or_write(project, &archives, &LockMode::Frozen).expect("frozen verify");
    }

    #[test]
    fn frozen_mode_refuses_mismatch() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        let a = write_archive(project, "a.a", b"alpha");
        verify_or_write(
            project,
            &[ArchiveEntry {
                package: "pkg-a".into(),
                target_key: "macos-arm64".into(),
                path: a.clone(),
            }],
            &LockMode::Default,
        )
        .expect("seed");
        write_archive(project, "a.a", b"alpha-v2");
        let err = verify_or_write(
            project,
            &[ArchiveEntry {
                package: "pkg-a".into(),
                target_key: "macos-arm64".into(),
                path: a,
            }],
            &LockMode::Frozen,
        )
        .expect_err("frozen+mismatch fails");
        assert!(err.to_string().contains("pkg-a"));
    }

    #[test]
    fn per_target_hashing_accumulates() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        let a_mac = write_archive(project, "a-macos.a", b"alpha-mac");
        let a_linux = write_archive(project, "a-linux.a", b"alpha-linux");
        let archives = vec![
            ArchiveEntry {
                package: "pkg-a".into(),
                target_key: "macos-arm64".into(),
                path: a_mac.clone(),
            },
            ArchiveEntry {
                package: "pkg-a".into(),
                target_key: "linux-x86_64".into(),
                path: a_linux.clone(),
            },
        ];
        let lock = verify_or_write(project, &archives, &LockMode::Default).expect("write");
        let pkg = &lock.native_library["pkg-a"];
        assert_eq!(pkg.sha256_per_target.len(), 2);
        assert_eq!(
            pkg.sha256_per_target["macos-arm64"],
            sha256_of_file(&a_mac).unwrap()
        );
        assert_eq!(
            pkg.sha256_per_target["linux-x86_64"],
            sha256_of_file(&a_linux).unwrap()
        );
    }

    #[test]
    fn lockfile_is_deterministic_across_writes() {
        let dir = tempdir().unwrap();
        let project = dir.path();
        let a = write_archive(project, "a.a", b"alpha");
        let b = write_archive(project, "b.a", b"beta");
        let archives = vec![
            ArchiveEntry {
                package: "pkg-z".into(),
                target_key: "linux-x86_64".into(),
                path: b,
            },
            ArchiveEntry {
                package: "pkg-a".into(),
                target_key: "macos-arm64".into(),
                path: a,
            },
        ];
        verify_or_write(project, &archives, &LockMode::Default).expect("first");
        let bytes_one = fs::read(project.join("perry.lock")).expect("read");
        fs::remove_file(project.join("perry.lock")).expect("rm");
        verify_or_write(project, &archives, &LockMode::Default).expect("second");
        let bytes_two = fs::read(project.join("perry.lock")).expect("read 2");
        assert_eq!(bytes_one, bytes_two);
    }

    #[test]
    fn lockmode_allows_refresh_matrix() {
        assert!(!LockMode::Default.allows_refresh("anything"));
        assert!(!LockMode::Frozen.allows_refresh("anything"));
        assert!(LockMode::Update(vec![]).allows_refresh("anything"));
        assert!(LockMode::Update(vec!["pkg-a".into()]).allows_refresh("pkg-a"));
        assert!(!LockMode::Update(vec!["pkg-a".into()]).allows_refresh("pkg-b"));
    }
}
