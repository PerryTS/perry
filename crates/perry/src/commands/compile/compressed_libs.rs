//! Transparent decompression of bundled, compressed static archives.
//!
//! The per-platform npm packages (`@perryts/perry-linux-arm64`, …) ship their
//! prebuilt static archives zstd-compressed (`libperry_runtime.a.zst`,
//! `libperry_stdlib.a.zst`, …) rather than raw `.a`, so the published tarball
//! stays under npm's registry upload limit. The uncompressed archives total
//! ~750 MB per platform; npm rejects the raw upload with HTTP 413 (Payload Too
//! Large). zstd brings the published package comfortably under the limit while
//! keeping every feature — the archives still contain the full stdlib so
//! out-of-tree `perry compile` can link any program.
//!
//! [`find_library_with_candidates`](super::library_search::find_library_with_candidates)
//! calls [`decompressed_archive`] when a candidate `.a` is absent but a sibling
//! `.a.zst` exists: the archive is decompressed once into a per-user cache and
//! the cached `.a` is linked. The cache slot is keyed on the compressed file's
//! size + mtime, so a new release lands in a fresh slot and a stale archive is
//! never served. Decompression is a one-time cost per machine per release;
//! later compiles reuse the cache. zstd is statically vendored into the perry
//! binary, so this adds no system-library dependency on the user's machine.
//!
//! This path is purely additive: installs that ship raw `.a` (Homebrew, apt,
//! in-tree dev builds) match a `.a` candidate first and never reach it.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};

/// Extension appended to a bundled archive to mark it zstd-compressed.
const COMPRESSED_EXT: &str = "zst";

/// `libperry_runtime.a` → `libperry_runtime.a.zst`.
pub(super) fn compressed_sibling(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".");
    name.push(COMPRESSED_EXT);
    PathBuf::from(name)
}

/// Decompress `compressed` (a `*.a.zst`) into the per-user cache and return the
/// path to the decompressed archive, reusing an existing cache entry when one
/// matches. `lib_name` is the canonical archive filename (e.g.
/// `libperry_runtime.a`); it is preserved in the cache so both full-path and
/// `-L<dir> -l<name>` link styles resolve the result.
pub(super) fn decompressed_archive(compressed: &Path, lib_name: &str) -> Result<PathBuf> {
    let meta =
        fs::metadata(compressed).with_context(|| format!("stat {}", compressed.display()))?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // One slot per (size, mtime): a different release (different bytes) gets a
    // distinct slot and can never shadow or be confused with a previous one.
    let slot = cache_root()?.join(format!("{:x}-{:x}", meta.len(), mtime));
    let out = slot.join(lib_name);
    if fs::metadata(&out).map(|m| m.len() > 0).unwrap_or(false) {
        return Ok(out);
    }

    fs::create_dir_all(&slot).with_context(|| format!("create cache dir {}", slot.display()))?;
    eprintln!(
        "  decompressing bundled {} (one-time; cached under {})",
        lib_name,
        slot.display()
    );

    // Decompress to a process-unique temp file, then atomically rename, so a
    // concurrent compile never links a half-written archive.
    let tmp = slot.join(format!(".{}.{}.tmp", lib_name, std::process::id()));
    let result = (|| -> Result<()> {
        let input =
            fs::File::open(compressed).with_context(|| format!("open {}", compressed.display()))?;
        // `Decoder::new` wraps the reader in its own `BufReader`.
        let mut decoder = zstd::Decoder::new(input)
            .with_context(|| format!("init zstd decoder for {}", compressed.display()))?;
        let mut out_file =
            fs::File::create(&tmp).with_context(|| format!("create {}", tmp.display()))?;
        io::copy(&mut decoder, &mut out_file)
            .with_context(|| format!("zstd-decompress {}", compressed.display()))?;
        // Propagate fsync failures (disk full, I/O error) so a truncated temp
        // file is cleaned up below rather than renamed into the cache.
        out_file
            .sync_all()
            .with_context(|| format!("flush {}", tmp.display()))?;
        Ok(())
    })();
    if let Err(e) = result {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }

    match fs::rename(&tmp, &out) {
        Ok(()) => Ok(out),
        Err(_) => {
            // Lost a race with a sibling process (or a cross-device rename):
            // use the finished archive if it now exists, else fail loudly.
            let _ = fs::remove_file(&tmp);
            if fs::metadata(&out).map(|m| m.len() > 0).unwrap_or(false) {
                Ok(out)
            } else {
                Err(anyhow!("failed to finalize decompressed {}", out.display()))
            }
        }
    }
}

/// Root directory for decompressed archives. Honors `PERRY_LIB_CACHE_DIR`,
/// otherwise the platform cache dir, otherwise the system temp dir.
fn cache_root() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("PERRY_LIB_CACHE_DIR") {
        if !dir.is_empty() {
            return Ok(PathBuf::from(dir));
        }
    }
    let base = dirs::cache_dir().unwrap_or_else(std::env::temp_dir);
    Ok(base.join("perry").join("libs"))
}
