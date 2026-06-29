//! Eval-mode `worker_threads` Worker support.
//!
//! Extracted from `collect_modules.rs` (file-size split). `new Worker(src,
//! { eval: true })` passes the worker SOURCE rather than a filename; this
//! materializes that source to a content-addressed temp `.js` file so the
//! existing file-worker machinery compiles it as a normal module.

use anyhow::{anyhow, Result};
use std::fs;

/// Write an eval-mode Worker's inline source to a content-addressed `.js` file
/// under the system temp dir and return its absolute path. Content addressing
/// keeps the path stable across compiles (so the object cache hits) and avoids
/// races between parallel lowering threads writing the same source.
pub(super) fn materialize_eval_worker_source(source: &str) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(source.as_bytes());
    let digest = hasher.finalize();
    let hex: String = digest.iter().take(16).map(|b| format!("{b:02x}")).collect();
    let dir = std::env::temp_dir().join("perry-eval-workers");
    fs::create_dir_all(&dir).map_err(|e| anyhow!("create {}: {}", dir.display(), e))?;
    let path = dir.join(format!("perry-eval-worker-{hex}.js"));
    // Content-addressed: only write when missing (or size differs) so
    // concurrent threads don't clobber an identical file mid-read.
    let needs_write = match fs::metadata(&path) {
        Ok(meta) => meta.len() != source.len() as u64,
        Err(_) => true,
    };
    if needs_write {
        fs::write(&path, source).map_err(|e| anyhow!("write {}: {}", path.display(), e))?;
    }
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
