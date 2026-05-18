//! #504 — binary attestation sidecar (MVP).
//!
//! When a Perry build is compiled with `--emit-attest`, the driver
//! writes `<binary>.attest.json` next to the executable containing
//! enough metadata to answer the question "did this binary come
//! from THIS source tree at THIS commit?":
//!
//! ```json
//! {
//!   "version": 1,
//!   "sha256": "abcd1234...",
//!   "size": 1048576,
//!   "perry_version": "0.5.999",
//!   "commit_sha": "0a1b2c3...",
//!   "built_at_unix": 1715990400,
//!   "binary_filename": "myapp"
//! }
//! ```
//!
//! The format is intentionally minimal — `version: 1` is a contract
//! for the matching `perry verify --attest` CLI side. Future
//! additions (CI signature blob, sigstore bundle, reproducible-builds
//! flags log) graft on under a new top-level key without bumping
//! the version.
//!
//! The full #504 acceptance criteria — published CI attestations,
//! tested reproducibility across machines, signed sigstore bundles —
//! are all tracked as follow-ups under #504. This MVP ships the
//! *primitive*: a per-build sidecar plus a verifier that proves
//! the binary hasn't been swapped post-build. CI publication +
//! determinism work can build on top.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

/// Versioned shape of `<binary>.attest.json`. Stable across Perry
/// releases — the `version` field lets future additions graft on
/// without breaking parsers in the wild.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AttestationManifest {
    pub version: u32,
    /// Hex-encoded SHA-256 of the binary bytes.
    pub sha256: String,
    /// Binary size in bytes (cross-check on top of the hash).
    pub size: u64,
    /// Perry version that produced this binary (from
    /// `env!("CARGO_PKG_VERSION")`).
    pub perry_version: String,
    /// Git commit SHA the build was rooted at, when available;
    /// empty string when the source tree isn't a git checkout.
    pub commit_sha: String,
    /// Unix epoch seconds at build time. *Not* a determinism
    /// claim — just provenance for "when did this build run?".
    pub built_at_unix: u64,
    /// `binary_path.file_name()`. Lets a verifier sanity-check it
    /// is reading the attestation that matches the binary file
    /// they're holding.
    pub binary_filename: String,
}

/// Compute SHA-256 of a file as `"<hex>"`. Streaming hash so very
/// large binaries don't OOM.
pub fn sha256_hex(path: &Path) -> Result<(String, u64)> {
    let mut file =
        std::fs::File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut total: u64 = 0;
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        total += n as u64;
    }
    Ok((format!("{:x}", hasher.finalize()), total))
}

/// Read `commit_sha` from the project's git tree. Best-effort:
/// returns the empty string when the build isn't inside a git
/// repo. The git2 crate isn't already a perry dep, so use the
/// `git` binary directly.
fn discover_commit_sha(project_root: &Path) -> String {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(project_root)
        .output();
    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        _ => String::new(),
    }
}

/// Build the manifest record for `binary_path`. Hashes the file
/// content and gathers provenance (perry version, git commit at
/// `project_root`, current Unix epoch). Returned separately from
/// the file write so tests can pin the shape without touching the
/// filesystem.
pub fn build_attestation(binary_path: &Path, project_root: &Path) -> Result<AttestationManifest> {
    let (sha256, size) = sha256_hex(binary_path)?;
    let perry_version = env!("CARGO_PKG_VERSION").to_string();
    let commit_sha = discover_commit_sha(project_root);
    let built_at_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let binary_filename = binary_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string();
    Ok(AttestationManifest {
        version: 1,
        sha256,
        size,
        perry_version,
        commit_sha,
        built_at_unix,
        binary_filename,
    })
}

/// Write the manifest to `<binary>.attest.json` alongside the
/// binary. Returns the resolved sidecar path.
pub fn write_attestation(
    binary_path: &Path,
    manifest: &AttestationManifest,
) -> Result<std::path::PathBuf> {
    let out = binary_path.with_extension("attest.json");
    let body = serde_json::to_string_pretty(manifest)
        .context("failed to serialize attestation manifest")?;
    std::fs::write(&out, body).with_context(|| format!("failed to write {}", out.display()))?;
    Ok(out)
}

/// Verify that `binary_path`'s on-disk SHA-256 + size match the
/// values recorded in `<binary>.attest.json`. Returns the loaded
/// manifest on success; bails with an actionable diagnostic on
/// mismatch or missing sidecar.
pub fn verify_against_sidecar(binary_path: &Path) -> Result<AttestationManifest> {
    let sidecar = binary_path.with_extension("attest.json");
    if !sidecar.exists() {
        bail!(
            "no attestation sidecar at {}.\n\
             \n\
             The attestation is produced by `perry compile --emit-attest`.\n\
             If you received this binary from a build CI, ask the publisher to\n\
             ship the matching `.attest.json` alongside the binary.",
            sidecar.display()
        );
    }
    let raw = std::fs::read_to_string(&sidecar)
        .with_context(|| format!("failed to read {}", sidecar.display()))?;
    let manifest: AttestationManifest = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", sidecar.display()))?;
    let (actual_hash, actual_size) = sha256_hex(binary_path)?;
    if actual_hash != manifest.sha256 || actual_size != manifest.size {
        bail!(
            "attestation MISMATCH for {}:\n\
             \n\
               expected sha256: {}\n\
               found    sha256: {}\n\
               expected size:   {}\n\
               found    size:   {}\n\
             \n\
             The binary on disk doesn't match the sidecar attestation.\n\
             This is the exact signal a swapped/tampered binary would produce.\n\
             Do NOT run it. (#504)",
            binary_path.display(),
            manifest.sha256,
            actual_hash,
            manifest.size,
            actual_size,
        );
    }
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_bin(body: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("myapp");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(body).unwrap();
        (dir, path)
    }

    #[test]
    fn sha256_hex_matches_known_vector() {
        let (_dir, path) = temp_bin(b"hello\n");
        let (h, n) = sha256_hex(&path).unwrap();
        assert_eq!(
            h,
            "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03"
        );
        assert_eq!(n, 6);
    }

    #[test]
    fn build_and_write_roundtrip() {
        let (dir, path) = temp_bin(b"compiled bytes");
        let manifest = build_attestation(&path, dir.path()).unwrap();
        assert_eq!(manifest.version, 1);
        assert_eq!(manifest.size, 14);
        assert_eq!(manifest.binary_filename, "myapp");
        assert_eq!(manifest.perry_version, env!("CARGO_PKG_VERSION"));

        let written = write_attestation(&path, &manifest).unwrap();
        assert!(written.ends_with("myapp.attest.json"));
        let raw = std::fs::read_to_string(&written).unwrap();
        // BTreeMap-free serde shape: presence checks instead of
        // strict order.
        assert!(raw.contains("\"version\": 1"));
        assert!(raw.contains("\"sha256\":"));
        assert!(raw.contains("\"size\": 14"));
    }

    #[test]
    fn verify_passes_when_hash_matches() {
        let (dir, path) = temp_bin(b"some-bytes");
        let m = build_attestation(&path, dir.path()).unwrap();
        write_attestation(&path, &m).unwrap();
        let read_back = verify_against_sidecar(&path).expect("verify must pass");
        assert_eq!(read_back, m);
    }

    #[test]
    fn verify_fails_when_binary_tampered() {
        let (dir, path) = temp_bin(b"original");
        let m = build_attestation(&path, dir.path()).unwrap();
        write_attestation(&path, &m).unwrap();
        // Swap binary contents — same path, different bytes.
        std::fs::write(&path, b"tampered").unwrap();
        let err = verify_against_sidecar(&path).expect_err("must detect mismatch");
        let msg = err.to_string();
        assert!(msg.contains("MISMATCH"), "{msg}");
        assert!(msg.contains("(#504)"), "{msg}");
    }

    #[test]
    fn verify_fails_when_sidecar_missing() {
        let (_dir, path) = temp_bin(b"no-sidecar");
        let err = verify_against_sidecar(&path).expect_err("must fail without sidecar");
        let msg = err.to_string();
        assert!(msg.contains("no attestation sidecar"), "{msg}");
        assert!(msg.contains("`perry compile --emit-attest`"), "{msg}");
    }

    #[test]
    fn manifest_is_pretty_printed_json() {
        // pretty-printed JSON makes `git diff` on a checked-in
        // attestation human-readable.
        let (dir, path) = temp_bin(b"x");
        let m = build_attestation(&path, dir.path()).unwrap();
        let raw = serde_json::to_string_pretty(&m).unwrap();
        assert!(raw.contains('\n'), "must be pretty-printed");
    }
}
