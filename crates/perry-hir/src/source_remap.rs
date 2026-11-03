//! Build-time remap of the absolute source paths baked into a compiled program.
//!
//! Perry folds each module's canonical absolute path into program-visible
//! values: `__dirname`, `__filename`, `import.meta.{url,dirname,filename}`, the
//! base of runtime `require` / `import.meta.resolve`, `Object.<anonymous> (...)`
//! stack-frame labels, the CommonJS `module.filename` / `require.cache` keys and
//! error messages that name a source location. Without a remap those values
//! carry the directory the program was *built* in, so the program only runs
//! where that directory exists.
//!
//! A [`SourceRemap`] rebases every path under `root` onto `target` (default
//! `/`): `<root>/node_modules/x/a.js` is baked as `/node_modules/x/a.js`.
//! Compile-time file I/O keeps using the real path; only baked values go
//! through [`baked_path`]. A path outside `root` is baked unchanged, so the
//! program stays correct (just not relocatable for that module); the driver
//! warns about such modules via [`is_covered`].
//!
//! The remap is process-global, installed by the driver for the duration of one
//! compile ([`install`]). With none installed [`baked_path`] returns its input
//! untouched, so output is byte-identical to a build without the option.

use std::borrow::Cow;
use std::path::{Component, Path, PathBuf};
use std::sync::RwLock;

static REMAP: RwLock<Option<SourceRemap>> = RwLock::new(None);

/// Rebase absolute source paths under `root` onto `target`.
#[derive(Debug, Clone)]
pub struct SourceRemap {
    root: PathBuf,
    /// `/`-separated absolute path with no trailing slash; empty means `/`.
    target: String,
}

impl SourceRemap {
    /// `root` must be canonical (the same form module paths are canonicalized
    /// to); `target` is the absolute path baked in place of `root`.
    pub fn new(root: PathBuf, target: &str) -> Self {
        let target = target.replace('\\', "/").trim_end_matches('/').to_string();
        Self { root, target }
    }

    /// The baked form of `path`, or `None` when it is outside `root`.
    pub fn rebase(&self, path: &Path) -> Option<String> {
        let relative = path.strip_prefix(&self.root).ok()?;
        let mut baked = self.target.clone();
        for component in relative.components() {
            if let Component::Normal(part) = component {
                baked.push('/');
                baked.push_str(&part.to_string_lossy());
            }
        }
        if baked.is_empty() {
            baked.push('/');
        }
        Some(baked)
    }
}

/// Keeps the installed remap alive; dropping it uninstalls the remap.
#[must_use = "the remap is uninstalled when the guard is dropped"]
pub struct SourceRemapGuard(());

impl Drop for SourceRemapGuard {
    fn drop(&mut self) {
        *REMAP.write().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

/// Install `remap` for every module lowered until the guard is dropped.
pub fn install(remap: SourceRemap) -> SourceRemapGuard {
    *REMAP.write().unwrap_or_else(|e| e.into_inner()) = Some(remap);
    SourceRemapGuard(())
}

/// The baked form of `real` under the installed remap; `None` when no remap is
/// installed or `real` is outside its root.
pub fn rebased(real: &Path) -> Option<String> {
    let guard = REMAP.read().unwrap_or_else(|e| e.into_inner());
    guard.as_ref().and_then(|r| r.rebase(real))
}

/// A string identifying the installed remap (`None` without one), for cache
/// keys: codegen bakes remapped path keys that the lowered HIR does not carry.
pub fn fingerprint() -> Option<String> {
    let guard = REMAP.read().unwrap_or_else(|e| e.into_inner());
    guard
        .as_ref()
        .map(|r| format!("{}\0{}", r.root.display(), r.target))
}

/// The path to bake into the program for the module at `real`.
pub fn baked_path(real: &str) -> Cow<'_, str> {
    match rebased(Path::new(real)) {
        Some(baked) => Cow::Owned(baked),
        None => Cow::Borrowed(real),
    }
}

/// [`baked_path`] for a [`Path`], lossily converted as the baked text always was.
pub fn baked_path_of(real: &Path) -> Cow<'_, str> {
    match rebased(real) {
        Some(baked) => Cow::Owned(baked),
        None => real.to_string_lossy(),
    }
}

/// Whether the installed remap covers `real`. `true` when none is installed,
/// since nothing is expected to be rebased then.
pub fn is_covered(real: &Path) -> bool {
    let guard = REMAP.read().unwrap_or_else(|e| e.into_inner());
    guard.as_ref().is_none_or(|r| r.rebase(real).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rebases_under_root_and_leaves_other_paths_alone() {
        let remap = SourceRemap::new(PathBuf::from("/home/runner/storage"), "/");
        let p = Path::new("/home/runner/storage/node_modules/x/dist/a.js");
        assert_eq!(
            remap.rebase(p).as_deref(),
            Some("/node_modules/x/dist/a.js")
        );
        assert_eq!(
            remap.rebase(Path::new("/home/runner/storage")).as_deref(),
            Some("/")
        );
        // A sibling that merely shares the textual prefix is not under root.
        assert_eq!(remap.rebase(Path::new("/home/runner/storage2/a.js")), None);
        assert_eq!(remap.rebase(Path::new("/opt/a.js")), None);
    }

    #[test]
    fn honors_a_non_root_target() {
        let remap = SourceRemap::new(PathBuf::from("/build/app"), "/srv/app/");
        assert_eq!(
            remap.rebase(Path::new("/build/app/dist/a.js")).as_deref(),
            Some("/srv/app/dist/a.js")
        );
    }
}
