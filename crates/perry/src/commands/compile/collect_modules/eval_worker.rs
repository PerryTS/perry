//! Eval-mode `worker_threads` Worker support.
//!
//! Extracted from `collect_modules.rs` (file-size split). `new Worker(src,
//! { eval: true })` passes the worker SOURCE rather than a filename; this
//! materializes that source to a content-addressed temp `.js` file so the
//! existing file-worker machinery compiles it as a normal module.

use anyhow::{anyhow, Result};
use std::fs;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};

// Per-process counter for unique temp filenames (no Date/rand needed).
static NEXT_TMP: AtomicU64 = AtomicU64::new(0);

/// Write an eval-mode Worker's inline source to a content-addressed `.js` file
/// under the system temp dir and return its absolute path. Content addressing
/// keeps the path stable across compiles (so the object cache hits).
///
/// Written atomically: a unique temp file is fully written then `rename`d into
/// the shared content-addressed path, so a concurrent rayon lowering thread can
/// never observe a half-written file, and reuse is gated on a full BYTE compare
/// (a size-only check could accept a truncated/corrupt same-length file).
/// Concurrent writers of the same source produce byte-identical content, so
/// whichever rename wins leaves a correct file.
pub(super) fn materialize_eval_worker_source(source: &str) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(source.as_bytes());
    let digest = hasher.finalize();
    let hex: String = digest.iter().take(16).map(|b| format!("{b:02x}")).collect();
    let dir = std::env::temp_dir().join("perry-eval-workers");
    fs::create_dir_all(&dir).map_err(|e| anyhow!("create {}: {}", dir.display(), e))?;
    let path = dir.join(format!("perry-eval-worker-{hex}.js"));

    // Reuse only if the existing file's bytes match exactly.
    if let Ok(existing) = fs::read(&path) {
        if existing == source.as_bytes() {
            return Ok(path.to_string_lossy().into_owned());
        }
    }

    // Write to a unique temp file, then atomically rename into place.
    let tmp = dir.join(format!(
        "perry-eval-worker-{hex}.{}.{}.tmp",
        std::process::id(),
        NEXT_TMP.fetch_add(1, Ordering::Relaxed)
    ));
    {
        let mut f =
            fs::File::create(&tmp).map_err(|e| anyhow!("create {}: {}", tmp.display(), e))?;
        f.write_all(source.as_bytes())
            .map_err(|e| anyhow!("write {}: {}", tmp.display(), e))?;
    }
    fs::rename(&tmp, &path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        anyhow!("rename {} -> {}: {}", tmp.display(), path.display(), e)
    })?;
    Ok(path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::materialize_eval_worker_source;

    #[test]
    fn materialize_is_deterministic_and_roundtrips() {
        let src = "\"use strict\";\nvar { parentPort } = require('worker_threads');\nparentPort.postMessage(1);\n";
        let p1 = materialize_eval_worker_source(src).expect("materialize");
        let p2 = materialize_eval_worker_source(src).expect("materialize again");
        // Content-addressed: identical source → identical path.
        assert_eq!(p1, p2);
        assert!(
            p1.ends_with(".js"),
            "worker file must be a .js module: {p1}"
        );
        let written = std::fs::read_to_string(&p1).expect("read back");
        assert_eq!(written, src);
        // Different source → different path.
        let other =
            materialize_eval_worker_source("console.log('x');\n").expect("materialize other");
        assert_ne!(p1, other);
    }
}
