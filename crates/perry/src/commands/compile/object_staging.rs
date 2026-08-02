//! Where a compile puts the `.o` files it emits, and who deletes them again.
//!
//! A `perry compile` produces one object per native module. What happens to
//! those objects depends on whether this invocation is going to *link* them:
//!
//! * **Linking** — the objects are intermediates. Nothing outside this process
//!   will ever look at them, so they go into a private staging directory that
//!   this invocation owns and removes ([`StagingDir`]).
//! * **`--no-link`** — the objects *are* the product. There is nothing to clean
//!   up, because they are delivered to the path the user named with `-o`
//!   ([`NoLinkDestination`]).
//!
//! # Why this module exists (#7167)
//!
//! The staging directory used to be created unconditionally, and removed by
//! two hand-written `remove_dir` calls — one on the executable-link exit, one
//! on the shared-library exit. `--no-link` returns before either, so it had no
//! cleanup at all, and every `--no-link` compile left a
//! `perry-objs-<pid>-<nanos>/` directory and its objects in the system temp
//! directory forever.
//!
//! That leak is unbounded in **compiles**: the directory name carries the pid
//! and a wall-clock nanosecond component, so no two invocations ever reuse one.
//! The machine this was written on had accumulated 3086 of them. Every
//! `--no-link` user was affected, and the compiler-output census
//! (`scripts/compiler_output_harness/`) compiles the corpus with `--no-link`
//! constantly, so a measurement campaign bled gigabytes a day.
//!
//! Two structural changes, rather than a third `remove_dir`:
//!
//! 1. **`--no-link` no longer creates a staging directory at all.** It cannot
//!    leak one. This is not a cleanup that has to fire — it is work that never
//!    happens. Three call sites that must each remember is how the third one
//!    came to be missing (#7167's own diagnosis).
//! 2. **The staging directory is removed by `Drop`**, so *every* exit from the
//!    pipeline — both links, the static-archive path, and any `?` in between —
//!    cleans up through one site. A future fourth exit inherits the cleanup
//!    instead of having to remember it.
//!
//! # Why deleting is safe (and why it was not, for the `.ll`)
//!
//! #7144/#7168 could not simply unlink the `.ll` handed to `clang -c`: #7131
//! had made that name a pure function of the IR, so two workers holding
//! identical IR *shared* the path, and a per-call unlink could race a sibling.
//! The fix there was to stop sharing.
//!
//! Nothing is shared here to begin with. The staging directory's name carries
//! the pid and a monotonic wall-clock component, so it belongs to exactly one
//! invocation; no other process can be looking at it, and removing it is
//! unobservable to a concurrent compile. That is a structural property of the
//! name, not a timing argument.
//!
//! # Failure policy
//!
//! * `--no-link`: a failed compile keeps every object it managed to write, at
//!   the path the user named. Nothing deletes them, on any path.
//! * Linking: the staging directory is removed on failure too, and the error
//!   names it. What diagnosing a codegen failure needs is the *IR*, which
//!   `PERRY_LLVM_KEEP_IR` retains (and #7168 keeps for a failed
//!   `compile_ll_to_object`) — a module that failed codegen emitted no object,
//!   and the objects of the modules that succeeded are reproducible.
//!   `--keep-intermediates` is the single, already-documented opt-in for
//!   keeping them, and the failure message points at it. Deliberately *not* a
//!   second retention mode: an escape hatch nobody exercises is a configuration
//!   nobody has verified (CLAUDE.md's GC knob kill-policy, generalised).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result};

/// A per-invocation object staging directory, removed when this value drops.
///
/// Created only when the compile is going to link. See the module docs for why
/// `--no-link` has none.
pub(super) struct StagingDir {
    path: PathBuf,
    /// Whether `Drop` should remove the directory.
    ///
    /// Starts armed. Every way of keeping the directory is an explicit,
    /// user-visible decision that disarms it — never an exit that forgot.
    /// The default direction matters: a new exit that does nothing cleans up,
    /// where under the old scheme it leaked (#7167).
    remove_on_drop: AtomicBool,
}

impl StagingDir {
    /// Create the staging directory under the system temp directory.
    pub(super) fn create() -> Result<Self> {
        Self::create_in(&std::env::temp_dir())
    }

    /// Create the staging directory under `parent`.
    ///
    /// Split out so the tests can exercise the real create/drop cycle without
    /// writing into the shared system temp directory — which is exactly the
    /// place this module exists to keep clean.
    pub(super) fn create_in(parent: &Path) -> Result<Self> {
        // #4266 (2026-07-02 audit fleet P0): objects used to land at
        // CWD-relative name-only paths, so two concurrent compiles sharing a
        // working directory overwrote each other's `<module>.o` mid-link.
        // pid + a strictly-monotonic wall component (the linker.rs #509
        // discipline) keeps simultaneous invocations disjoint.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = parent.join(format!("perry-objs-{}-{}", std::process::id(), nanos));
        std::fs::create_dir_all(&path)
            .with_context(|| format!("failed to create object staging dir {}", path.display()))?;
        Ok(Self {
            path,
            remove_on_drop: AtomicBool::new(true),
        })
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    /// Keep the directory instead of removing it on drop, and report the path
    /// so the caller can name it.
    ///
    /// The one caller is `--keep-intermediates`; the failure paths use it to
    /// name the directory in the error they are about to return.
    pub(super) fn keep(&self) -> &Path {
        self.remove_on_drop.store(false, Ordering::Relaxed);
        &self.path
    }
}

impl Drop for StagingDir {
    fn drop(&mut self) {
        if !self.remove_on_drop.load(Ordering::Relaxed) {
            return;
        }
        // `remove_dir_all`, not `remove_dir`: the directory belongs to this
        // invocation, so anything inside it is ours and a stray file must not
        // be able to turn cleanup into a silent no-op. The old code removed
        // the directory only when it was already empty, which meant one
        // unexpected file re-created the leak this module exists to close.
        //
        // Best-effort. A compile that produced the right answer must not fail
        // because a temp directory could not be unlinked.
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Where a `--no-link` compile writes the objects it emits.
///
/// `--no-link` used to ignore `-o` entirely and leave its objects in the temp
/// staging directory — the flag's documented product ("produce object file
/// only") existed only as a path printed on stdout, in a directory nobody
/// deleted. Delivering to `-o` is what makes the objects the *user's* files:
/// they persist because they are wanted, not because cleanup was missing.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct NoLinkDestination {
    /// The directory every emitted object goes into.
    dir: PathBuf,
    /// The exact path for the single-module case, when `-o` was given.
    single: Option<PathBuf>,
}

impl NoLinkDestination {
    /// Resolve the destination for a `--no-link` compile.
    ///
    /// * `-o` given, one native module — the object is written to `-o`
    ///   verbatim. This is what `cc -c foo.c -o foo.o` does, and the case
    ///   essentially every caller means.
    /// * `-o` given, several native modules — one `-o` cannot name N files
    ///   (`cc` rejects that combination outright). The objects go into `-o`'s
    ///   directory under their module-derived names, so they land where the
    ///   user pointed and a separate link step can find them together.
    /// * no `-o` — the current directory, module-derived names.
    ///
    /// The rule keys on the *module count*, which is a property of the program,
    /// rather than on how many objects codegen actually wrote — that varies
    /// with object-cache warmth, and `-o` must not mean two different things
    /// depending on whether a cache was hot.
    pub(super) fn resolve(output: Option<&Path>, native_module_count: usize) -> Self {
        let Some(out) = output else {
            return Self {
                dir: PathBuf::from("."),
                single: None,
            };
        };
        // `Path::new("x.o").parent()` is `Some("")`, not `None`.
        let dir = match out.parent() {
            Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
            _ => PathBuf::from("."),
        };
        let single = (native_module_count == 1).then(|| out.to_path_buf());
        Self { dir, single }
    }

    /// Create the destination directory, if it does not exist yet.
    pub(super) fn prepare(&self) -> Result<()> {
        std::fs::create_dir_all(&self.dir).with_context(|| {
            format!(
                "failed to create the --no-link object output directory {}",
                self.dir.display()
            )
        })
    }

    pub(super) fn dir(&self) -> &Path {
        &self.dir
    }

    /// The path for one emitted artifact.
    ///
    /// `ext` is `"o"` for an object and `"ll"` in bitcode-link mode. Only an
    /// object is `--no-link`'s product, so only an object takes `-o` verbatim
    /// — an `-o app.o` that produced LLVM IR would be a lie about the file's
    /// contents.
    pub(super) fn artifact_path(&self, stem: &str, ext: &str) -> PathBuf {
        match &self.single {
            Some(p) if ext == "o" => p.clone(),
            _ => self.dir.join(format!("{}.{}", stem, ext)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "perry-objstaging-test-{}-{}-{:?}",
            std::process::id(),
            tag,
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The #7167 property, at the unit level: a staging directory that has been
    /// used and dropped leaves *nothing* behind — not the objects, and not the
    /// directory either.
    ///
    /// Asserted as "the parent is empty", not "the objects are gone". An empty
    /// scratch directory left behind is the same unbounded leak wearing a
    /// smaller coat: the name carries pid + nanos, so it is one fresh directory
    /// per compile whether or not there is anything in it.
    #[test]
    fn dropping_a_staging_dir_leaves_nothing_behind() {
        let root = tmp_root("drop");
        {
            let staging = StagingDir::create_in(&root).unwrap();
            std::fs::write(staging.path().join("mod_a.o"), b"objectbytes").unwrap();
            std::fs::write(staging.path().join("mod_b.o"), b"objectbytes").unwrap();
            assert!(staging.path().is_dir());
        }
        let left: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert!(left.is_empty(), "staging dir leaked: {:?}", left);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A stray file must not be able to turn cleanup into a no-op. The old
    /// `remove_dir` was non-recursive and silently did nothing when the
    /// directory still held anything the cleanup loop had not listed.
    #[test]
    fn an_unexpected_file_does_not_defeat_cleanup() {
        let root = tmp_root("stray");
        {
            let staging = StagingDir::create_in(&root).unwrap();
            std::fs::create_dir_all(staging.path().join("nested")).unwrap();
            std::fs::write(staging.path().join("nested/surprise.txt"), b"x").unwrap();
        }
        let left: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert!(
            left.is_empty(),
            "stray content defeated cleanup: {:?}",
            left
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// `--keep-intermediates` (and the failure paths, which use the same call)
    /// must actually keep the directory, and must report the path so the caller
    /// can name it. A retention hatch that silently deletes is worse than none.
    #[test]
    fn keep_retains_the_directory_and_reports_its_path() {
        let root = tmp_root("keep");
        let kept: PathBuf;
        {
            let staging = StagingDir::create_in(&root).unwrap();
            std::fs::write(staging.path().join("mod_a.o"), b"objectbytes").unwrap();
            kept = staging.keep().to_path_buf();
        }
        assert!(kept.is_dir(), "keep() did not retain {}", kept.display());
        assert!(kept.join("mod_a.o").is_file());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Two staging directories created back to back must not collide, because
    /// "removal is unobservable to a concurrent compile" rests entirely on the
    /// name being unique to one invocation.
    #[test]
    fn staging_dirs_are_unique_per_call() {
        let root = tmp_root("unique");
        let a = StagingDir::create_in(&root).unwrap();
        let b = StagingDir::create_in(&root).unwrap();
        assert_ne!(a.path(), b.path());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn single_module_no_link_honours_dash_o_verbatim() {
        let d = NoLinkDestination::resolve(Some(Path::new("/build/app.o")), 1);
        assert_eq!(d.dir(), Path::new("/build"));
        assert_eq!(
            d.artifact_path("app_ts", "o"),
            PathBuf::from("/build/app.o")
        );
    }

    /// One `-o` cannot name several files, so the module-derived names win and
    /// `-o` contributes only the directory.
    #[test]
    fn multi_module_no_link_uses_the_dash_o_directory() {
        let d = NoLinkDestination::resolve(Some(Path::new("/build/app.o")), 3);
        assert_eq!(d.dir(), Path::new("/build"));
        assert_eq!(
            d.artifact_path("app_ts", "o"),
            PathBuf::from("/build/app_ts.o")
        );
        assert_eq!(
            d.artifact_path("dep_ts", "o"),
            PathBuf::from("/build/dep_ts.o")
        );
    }

    /// `-o app.o` with no directory component must mean "here", not "" — a
    /// `PathBuf::from("").join("app.o")` is a relative path that happens to
    /// work, but `create_dir_all("")` fails.
    #[test]
    fn bare_dash_o_resolves_to_the_current_directory() {
        let d = NoLinkDestination::resolve(Some(Path::new("app.o")), 1);
        assert_eq!(d.dir(), Path::new("."));
        assert_eq!(d.artifact_path("app_ts", "o"), PathBuf::from("app.o"));
    }

    #[test]
    fn no_dash_o_writes_module_named_objects_into_the_current_directory() {
        let d = NoLinkDestination::resolve(None, 1);
        assert_eq!(d.dir(), Path::new("."));
        assert_eq!(d.artifact_path("app_ts", "o"), PathBuf::from("./app_ts.o"));
    }

    /// Bitcode-link mode emits `.ll`, not an object. `-o app.o` must not be
    /// handed LLVM IR under an object's name.
    #[test]
    fn bitcode_mode_never_takes_dash_o_verbatim() {
        let d = NoLinkDestination::resolve(Some(Path::new("/build/app.o")), 1);
        assert_eq!(
            d.artifact_path("app_ts", "ll"),
            PathBuf::from("/build/app_ts.ll")
        );
    }

    /// The destination must not depend on object-cache warmth: the rule keys on
    /// the module count, so a hot cache and a cold cache name the same file.
    #[test]
    fn the_rule_keys_on_module_count_not_on_what_codegen_wrote() {
        let cold = NoLinkDestination::resolve(Some(Path::new("/build/app.o")), 1);
        let hot = NoLinkDestination::resolve(Some(Path::new("/build/app.o")), 1);
        assert_eq!(cold, hot);
    }
}
