//! Checkpoints for completed LLVM units of an otherwise unfinished module.
//!
//! The driver supplies a directory namespaced by its full object-cache key
//! (compiler executable, HIR, target, options and codegen environment). Native
//! emission additionally fingerprints each unit's actual frozen input. Neither
//! a unit index alone nor a source-file name is a sufficient cache identity.

use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

thread_local! {
    static DIRECTORY: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// Scope checkpoints to one module compilation on the calling thread. `None`
/// disables them (including inside an outer scope). The directory MUST include
/// the driver's complete object-cache identity; it is not just a cache root.
/// Workers receive an owned snapshot, never another worker's thread-local state.
pub fn with_module_cache<T>(directory: Option<PathBuf>, compile: impl FnOnce() -> T) -> T {
    struct Restore(Option<PathBuf>);
    impl Drop for Restore {
        fn drop(&mut self) {
            DIRECTORY.with(|directory| directory.replace(self.0.take()));
        }
    }
    let _restore = Restore(DIRECTORY.with(|slot| slot.replace(directory)));
    compile()
}

#[derive(Clone)]
pub(crate) struct UnitCache(PathBuf);

fn checksum(bytes: &[u8]) -> u64 {
    let mut hash = DefaultHasher::new();
    hash.write(bytes);
    hash.finish()
}

impl UnitCache {
    pub(crate) fn current() -> Option<Self> {
        DIRECTORY.with(|directory| directory.borrow().clone().map(Self))
    }

    fn path(&self, index: usize, total: usize, input: u64) -> PathBuf {
        self.0.join(format!("{index}-{total}-{input:016x}.puc"))
    }

    pub(crate) fn load(&self, index: usize, total: usize, input: u64) -> Option<Vec<u8>> {
        let mut bytes = std::fs::read(self.path(index, total, input)).ok()?;
        // A single atomically published record prevents torn object/metadata
        // pairs. Reject truncation and corruption as misses, never as objects.
        if bytes.len() <= 24 || &bytes[..8] != b"PERRYUC1" {
            return None;
        }
        let length = u64::from_le_bytes(bytes[8..16].try_into().ok()?);
        let digest = u64::from_le_bytes(bytes[16..24].try_into().ok()?);
        if length != (bytes.len() - 24) as u64 || checksum(&bytes[24..]) != digest {
            return None;
        }
        bytes.drain(..24);
        Some(bytes)
    }

    pub(crate) fn store(&self, index: usize, total: usize, input: u64, bytes: &[u8]) {
        // Caching is best effort: disk-full/read-only caches must not change
        // compilation semantics. Unique create_new files also permit parallel
        // invocations compiling the same module without publishing partial data.
        static NEXT: AtomicU64 = AtomicU64::new(0);
        if bytes.is_empty() || std::fs::create_dir_all(&self.0).is_err() {
            return;
        }
        let temporary = self.0.join(format!(
            ".unit-{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let Ok(mut file) = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        else {
            return;
        };
        let written = (|| -> std::io::Result<()> {
            file.write_all(b"PERRYUC1")?;
            file.write_all(&(bytes.len() as u64).to_le_bytes())?;
            file.write_all(&checksum(bytes).to_le_bytes())?;
            file.write_all(bytes)?;
            file.sync_all()
        })();
        drop(file);
        if written.is_err() || std::fs::rename(&temporary, self.path(index, total, input)).is_err()
        {
            let _ = std::fs::remove_file(&temporary);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> UnitCache {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "perry-unit-cache-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        UnitCache(path)
    }

    #[test]
    fn incomplete_module_retains_only_completed_matching_units() {
        let cache = fixture();
        cache.store(0, 3, 42, b"finished object");
        let resumed = UnitCache(cache.0.clone());
        assert_eq!(resumed.load(0, 3, 42).unwrap(), b"finished object");
        assert!(resumed.load(1, 3, 42).is_none());
        assert!(resumed.load(0, 4, 42).is_none());
        assert!(resumed.load(0, 3, 43).is_none());
        assert!(UnitCache(cache.0.join("different-compiler"))
            .load(0, 3, 42)
            .is_none());
        std::fs::remove_dir_all(cache.0).unwrap();
    }

    #[test]
    fn corrupt_or_truncated_checkpoints_are_misses() {
        let cache = fixture();
        cache.store(0, 2, 42, b"finished object");
        let path = cache.path(0, 2, 42);
        let original = std::fs::read(&path).unwrap();
        for length in 0..original.len() {
            std::fs::write(&path, &original[..length]).unwrap();
            assert!(cache.load(0, 2, 42).is_none());
        }
        let mut corrupt = original;
        corrupt[24] ^= 1;
        std::fs::write(&path, corrupt).unwrap();
        assert!(cache.load(0, 2, 42).is_none());
        cache.store(0, 2, 42, b"recompiled object");
        assert_eq!(cache.load(0, 2, 42).unwrap(), b"recompiled object");
        std::fs::remove_dir_all(cache.0).unwrap();
    }

    #[test]
    fn scope_is_nested_disabled_and_thread_local() {
        let cache = fixture();
        assert!(UnitCache::current().is_none());
        with_module_cache(Some(cache.0.clone()), || {
            assert_eq!(UnitCache::current().unwrap().0, cache.0);
            with_module_cache(None, || assert!(UnitCache::current().is_none()));
            assert_eq!(UnitCache::current().unwrap().0, cache.0);
            std::thread::spawn(|| assert!(UnitCache::current().is_none()))
                .join()
                .unwrap();
        });
        assert!(UnitCache::current().is_none());
        std::fs::remove_dir_all(cache.0).unwrap();
    }

    #[test]
    fn unwinding_restores_scope_and_unwritable_cache_is_nonfatal() {
        let cache = fixture();
        let _ = std::panic::catch_unwind(|| {
            with_module_cache(Some(cache.0.clone()), || panic!("fixture"));
        });
        assert!(UnitCache::current().is_none());
        let path = cache.0.join("not-a-directory");
        std::fs::write(&path, b"file").unwrap();
        let unusable = UnitCache(path);
        unusable.store(0, 2, 42, b"object");
        assert!(unusable.load(0, 2, 42).is_none());
        std::fs::remove_dir_all(cache.0).unwrap();
    }
}
