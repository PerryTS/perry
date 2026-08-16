//! The panic-strategy contract for every profile — in *every* workspace in
//! this repository — that builds a runtime archive Perry links into compiled
//! programs (#7302, #8034).
//!
//! The exception transport requires the unwinder to step *through* runtime
//! Rust frames with `longjmp`-equivalent semantics. Under
//! `panic = "unwind"` rustc plants RFC-2945 abort-on-unwind guards in every
//! `extern "C"` function that contains an interior Rust call, so a JS throw
//! crossing such a helper — a throwing getter, a `JSON.parse` error, a
//! throwing `map` callback — aborts the process instead of being caught.
//! The give-away in a crash report is `panic in a function that cannot
//! unwind` immediately below `_js_throw`, *with a handler armed*
//! (`try_depth` > 0).
//!
//! This is not hypothetical and it is not caught by any other gate. It has
//! now shipped three times:
//!
//! 1. `[profile.release]` itself was on the default (`unwind`) — closed by
//!    #7302.
//! 2. `[profile.dist]` (what `release-packages.yml` builds the SHIPPED
//!    `libperry_{runtime,stdlib}.a` with) *inherits* `release` but then
//!    re-declared `panic = "unwind"`, which wins. Every local build, every
//!    CI job and the whole parity suite were correct while the artifact
//!    users install aborted on the first cross-helper throw.
//! 3. A **separate workspace** in the tree —
//!    `tests/release/packages/next-app-route/provider/Cargo.toml` — set
//!    `codegen-units`/`lto`/`strip` in its `[profile.release]` and simply
//!    never mentioned `panic`, silently taking cargo's `unwind` default.
//!    The first two instances were caught by a check that read only the
//!    main workspace manifest, so this one was invisible to it.
//!
//! Nothing failed to compile. Nothing went red. Instance 3 cost most of a
//! session to diagnose.
//!
//! So the invariant is asserted here, in `cargo-test` (per PR), by reading
//! *every* manifest in the repository:
//!
//! * A workspace root whose member graph can reach `perry-runtime` or
//!   `perry-stdlib` through a **path** dependency is "runtime-building".
//! * Its `[profile.release]` must resolve to `panic = "abort"`. **An absent
//!   `panic` key is a failure, not a pass** — that is precisely instance 3:
//!   cargo's default is `unwind`, so silence is the bug.
//! * Any other profile it declares must not name a non-`abort` strategy —
//!   `inherits` is not accepted as evidence, because the failure mode is
//!   exactly an override that looks harmless (instance 2).
//!
//! Cargo semantics the audit deliberately mirrors rather than grepping for:
//!
//! * **Only a workspace ROOT's profiles are read.** A `[profile.*]` block in
//!   a non-root member manifest is ignored by cargo (it warns). Checking
//!   those would produce false alarms; *not* checking a root would miss the
//!   real thing.
//! * **A package finds its root by walking up** to the nearest manifest with
//!   a `[workspace]` table that does not `exclude` it (or via an explicit
//!   `package.workspace` pointer), so the audit attributes members the same
//!   way instead of resolving `members` globs.
//! * **`inherits` chains are followed**, so `[profile.dist] inherits =
//!   "release"` is judged by what it actually resolves to.
//! * **`dev-dependencies` are not an edge.** A dev-dependency is linked only
//!   into test/bench harnesses, and cargo ignores `panic` for those profiles
//!   outright — such a workspace cannot emit a shipped runtime archive.
//! * **Optional dependencies ARE an edge.** Whether the feature is on
//!   depends on the build invocation, and the remedy (one `panic = "abort"`
//!   line) is harmless for a workspace that never enables it.
//!
//! The walk covers the whole repository except `target/` and dot-directories
//! (`.git`, and `.claude/worktrees`, which is gitignored scratch). It does
//! *not* skip `node_modules`: a compile-as-package runtime provider can
//! legitimately live under such a path, and pruning it is how you get a
//! fourth instance. An unparseable manifest is skipped — cargo could not
//! build it either, so it ships nothing.

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::{Component, Path, PathBuf};

    use toml::Table;

    /// Package names whose object code *is* the runtime archive. Reaching
    /// either one through path dependencies makes a workspace subject to the
    /// contract.
    const RUNTIME_PACKAGES: &[&str] = &["perry-runtime", "perry-stdlib"];

    /// Directory names never descended into. `target` is build output;
    /// dot-directories are `.git` plus agent scratch (`.claude/worktrees` is
    /// gitignored). Everything else — including `node_modules` — is walked.
    const PRUNED_DIRS: &[&str] = &["target"];

    /// Profiles the main workspace ships runtime archives with, asserted by
    /// name so a rename cannot silently drop one from the audit.
    const MAIN_SHIPPING_PROFILES: &[&str] = &["release", "dist", "perry-dev"];

    fn repo_root() -> PathBuf {
        let path = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        path.canonicalize()
            .unwrap_or_else(|e| panic!("repo root unresolvable at {}: {e}", path.display()))
    }

    // ---------------------------------------------------------------- model

    /// One parsed `Cargo.toml`, keyed by its own directory.
    struct Manifest {
        /// Directory containing the manifest.
        dir: PathBuf,
        /// Path relative to the repo root, for messages.
        rel: String,
        doc: Table,
    }

    impl Manifest {
        fn package_name(&self) -> Option<&str> {
            self.doc.get("package")?.get("name")?.as_str()
        }

        fn is_workspace_root(&self) -> bool {
            self.doc.contains_key("workspace")
        }

        fn workspace_table(&self) -> Option<&Table> {
            self.doc.get("workspace")?.as_table()
        }

        /// `[workspace] exclude = [...]` — paths relative to this root.
        fn excludes(&self, dir: &Path) -> bool {
            let Some(ws) = self.workspace_table() else {
                return false;
            };
            let Some(list) = ws.get("exclude").and_then(|v| v.as_array()) else {
                return false;
            };
            list.iter()
                .filter_map(|v| v.as_str())
                .any(|entry| dir.starts_with(normalize(&self.dir.join(entry))))
        }

        /// An explicit `[package] workspace = "../.."` pointer.
        fn explicit_workspace(&self) -> Option<PathBuf> {
            let p = self.doc.get("package")?.get("workspace")?.as_str()?;
            Some(normalize(&self.dir.join(p)))
        }
    }

    /// Lexical path normalization — `canonicalize` is not usable because a
    /// dependency `path` may point at a directory that does not exist (the
    /// package name alone can already decide the question).
    fn normalize(p: &Path) -> PathBuf {
        let mut out = PathBuf::new();
        for c in p.components() {
            match c {
                Component::ParentDir => {
                    out.pop();
                }
                Component::CurDir => {}
                other => out.push(other.as_os_str()),
            }
        }
        out
    }

    // ------------------------------------------------------------ discovery

    fn collect_manifests(root: &Path) -> BTreeMap<PathBuf, Manifest> {
        let mut out = BTreeMap::new();
        let walker = walkdir::WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                if e.depth() == 0 {
                    return true;
                }
                let name = e.file_name().to_string_lossy();
                if !e.file_type().is_dir() {
                    return true;
                }
                !name.starts_with('.') && !PRUNED_DIRS.contains(&name.as_ref())
            });

        for entry in walker.filter_map(Result::ok) {
            if !entry.file_type().is_file() || entry.file_name() != "Cargo.toml" {
                continue;
            }
            let path = entry.path();
            let Ok(text) = std::fs::read_to_string(path) else {
                continue;
            };
            // An unparseable manifest builds nothing; cargo would reject it
            // too, so it cannot ship a runtime archive.
            let Ok(doc) = text.parse::<Table>() else {
                continue;
            };
            let dir = normalize(path.parent().expect("Cargo.toml has a parent"));
            let rel = dir
                .strip_prefix(root)
                .map(|r| {
                    if r.as_os_str().is_empty() {
                        "Cargo.toml".to_string()
                    } else {
                        format!("{}/Cargo.toml", r.display())
                    }
                })
                .unwrap_or_else(|_| path.display().to_string());
            out.insert(dir.clone(), Manifest { dir, rel, doc });
        }
        out
    }

    /// Cargo's own root-finding: walk up to the nearest manifest carrying a
    /// `[workspace]` table that does not `exclude` this package.
    fn workspace_root_of(dir: &Path, manifests: &BTreeMap<PathBuf, Manifest>) -> Option<PathBuf> {
        let me = manifests.get(dir)?;
        if let Some(explicit) = me.explicit_workspace() {
            if manifests.contains_key(&explicit) {
                return Some(explicit);
            }
        }
        if me.is_workspace_root() {
            return Some(dir.to_path_buf());
        }
        let mut cursor = dir.parent();
        while let Some(anc) = cursor {
            if let Some(m) = manifests.get(anc) {
                if m.is_workspace_root() && !m.excludes(dir) {
                    return Some(anc.to_path_buf());
                }
            }
            cursor = anc.parent();
        }
        None
    }

    // ----------------------------------------------------- dependency edges

    /// Path dependencies declared by `manifest`, as `(package name, dir)`.
    ///
    /// `dev-dependencies` are deliberately excluded (see module docs).
    /// `foo.workspace = true` is resolved against the owning workspace root's
    /// `[workspace.dependencies]`, where the `path` is relative to the ROOT.
    fn path_dependencies(
        manifest: &Manifest,
        ws_root: Option<&Manifest>,
    ) -> Vec<(String, PathBuf)> {
        let mut out = Vec::new();
        let mut visit = |table: &Table, base: &Path| {
            for (key, value) in table {
                let Some(entry) = value.as_table() else {
                    continue; // `foo = "1.0"` — a registry dep.
                };
                if entry.get("workspace").and_then(|v| v.as_bool()) == Some(true) {
                    if let Some(root) = ws_root {
                        let inherited = root
                            .workspace_table()
                            .and_then(|w| w.get("dependencies"))
                            .and_then(|d| d.as_table())
                            .and_then(|d| d.get(key))
                            .and_then(|e| e.as_table());
                        if let Some(inherited) = inherited {
                            if let Some(p) = inherited.get("path").and_then(|v| v.as_str()) {
                                let name = inherited
                                    .get("package")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or(key);
                                out.push((name.to_string(), normalize(&root.dir.join(p))));
                            }
                        }
                    }
                    continue;
                }
                if let Some(p) = entry.get("path").and_then(|v| v.as_str()) {
                    let name = entry.get("package").and_then(|v| v.as_str()).unwrap_or(key);
                    out.push((name.to_string(), normalize(&base.join(p))));
                }
            }
        };

        for section in ["dependencies", "build-dependencies"] {
            if let Some(t) = manifest.doc.get(section).and_then(|v| v.as_table()) {
                visit(t, &manifest.dir);
            }
        }
        // `[target.'cfg(...)'.dependencies]`
        if let Some(targets) = manifest.doc.get("target").and_then(|v| v.as_table()) {
            for cfg in targets.values() {
                let Some(cfg) = cfg.as_table() else { continue };
                for section in ["dependencies", "build-dependencies"] {
                    if let Some(t) = cfg.get(section).and_then(|v| v.as_table()) {
                        visit(t, &manifest.dir);
                    }
                }
            }
        }
        out
    }

    /// Does any member of `root` reach a runtime package? Returns the
    /// shortest witness chain, for the failure message.
    fn runtime_witness(
        members: &[PathBuf],
        manifests: &BTreeMap<PathBuf, Manifest>,
        roots: &BTreeMap<PathBuf, PathBuf>,
    ) -> Option<Vec<String>> {
        let mut queue: std::collections::VecDeque<(PathBuf, Vec<String>)> = members
            .iter()
            .map(|m| {
                let label = manifests
                    .get(m)
                    .and_then(|x| x.package_name())
                    .map(str::to_string)
                    .unwrap_or_else(|| manifests.get(m).map(|x| x.rel.clone()).unwrap_or_default());
                (m.clone(), vec![label])
            })
            .collect();
        let mut seen: BTreeSet<PathBuf> = members.iter().cloned().collect();

        while let Some((dir, chain)) = queue.pop_front() {
            let Some(m) = manifests.get(&dir) else {
                continue;
            };
            if let Some(name) = m.package_name() {
                if RUNTIME_PACKAGES.contains(&name) {
                    return Some(chain);
                }
            }
            let owner = roots.get(&dir).and_then(|r| manifests.get(r));
            for (name, dep_dir) in path_dependencies(m, owner) {
                let mut next = chain.clone();
                next.push(name.clone());
                if RUNTIME_PACKAGES.contains(&name.as_str()) {
                    return Some(next);
                }
                if manifests.contains_key(&dep_dir) && seen.insert(dep_dir.clone()) {
                    queue.push_back((dep_dir, next));
                }
            }
        }
        None
    }

    // ------------------------------------------------------------- profiles

    /// Where a profile's effective `panic` value comes from.
    enum Panic {
        /// Declared as `value` by `[profile.<by>]`.
        Declared { value: String, by: String },
        /// Nothing in the `inherits` chain declares it: cargo's default,
        /// which is `unwind` for every built-in profile.
        CargoDefault,
    }

    fn profiles_of(root: &Manifest) -> Option<&Table> {
        root.doc.get("profile")?.as_table()
    }

    fn resolve_panic(profiles: Option<&Table>, name: &str) -> Panic {
        let Some(profiles) = profiles else {
            return Panic::CargoDefault;
        };
        let mut cursor = name.to_string();
        let mut seen = BTreeSet::new();
        while seen.insert(cursor.clone()) {
            let Some(p) = profiles.get(&cursor).and_then(|v| v.as_table()) else {
                return Panic::CargoDefault;
            };
            if let Some(v) = p.get("panic").and_then(|v| v.as_str()) {
                return Panic::Declared {
                    value: v.to_string(),
                    by: cursor,
                };
            }
            match p.get("inherits").and_then(|v| v.as_str()) {
                Some(parent) => cursor = parent.to_string(),
                None => return Panic::CargoDefault,
            }
        }
        Panic::CargoDefault
    }

    // ---------------------------------------------------------------- audit

    struct Audit {
        violations: Vec<String>,
        /// Repo-relative manifest paths of the runtime-building workspace
        /// roots the walk classified. Used as the liveness signal: an empty
        /// list means the audit examined nothing and would pass vacuously.
        runtime_workspaces: Vec<String>,
        manifests_seen: usize,
    }

    fn audit(root: &Path) -> Audit {
        let root = normalize(root);
        let manifests = collect_manifests(&root);

        // Attribute every manifest to its workspace root.
        let mut roots: BTreeMap<PathBuf, PathBuf> = BTreeMap::new();
        for dir in manifests.keys() {
            if let Some(r) = workspace_root_of(dir, &manifests) {
                roots.insert(dir.clone(), r);
            }
        }
        let mut members: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
        for (dir, ws) in &roots {
            // A virtual manifest (no `[package]`) is not itself a member.
            if manifests.get(dir).and_then(|m| m.package_name()).is_some() || dir != ws {
                members.entry(ws.clone()).or_default().push(dir.clone());
            }
        }

        let mut violations = Vec::new();
        let mut runtime_workspaces = Vec::new();

        for (ws_dir, ws_members) in &members {
            let Some(ws) = manifests.get(ws_dir) else {
                continue;
            };
            let Some(chain) = runtime_witness(ws_members, &manifests, &roots) else {
                continue;
            };
            runtime_workspaces.push(ws.rel.clone());

            let profiles = profiles_of(ws);
            let via = chain.join(" -> ");

            // 1. `[profile.release]` must resolve to abort. Silence is the
            //    bug (instance 3), so CargoDefault fails here.
            match resolve_panic(profiles, "release") {
                Panic::Declared { ref value, .. } if value.as_str() == "abort" => {}
                Panic::Declared { value, by } => violations.push(fix_message(
                    &ws.rel,
                    &via,
                    &format!(
                        "[profile.{by}] declares panic = \"{value}\", so `release` resolves to \
                         \"{value}\""
                    ),
                )),
                Panic::CargoDefault => {
                    let detail = if profiles
                        .and_then(|p| p.get("release"))
                        .and_then(|p| p.as_table())
                        .is_some()
                    {
                        "its [profile.release] declares no `panic` key, so it silently takes \
                         cargo's default, \"unwind\""
                    } else {
                        "it declares no [profile.release] at all, so `release` takes cargo's \
                         default, \"unwind\""
                    };
                    violations.push(fix_message(&ws.rel, &via, detail));
                }
            }

            // 2. No profile declared here may name a non-abort strategy.
            //    `inherits` is not evidence — instance 2 was an override that
            //    looked harmless under an `inherits = "release"` line.
            if let Some(profiles) = profiles {
                for name in profiles.keys() {
                    if name == "release" {
                        continue; // Already judged, with a better message.
                    }
                    if let Panic::Declared { value, by } = resolve_panic(Some(profiles), name) {
                        if value != "abort" {
                            violations.push(fix_message(
                                &ws.rel,
                                &via,
                                &format!(
                                    "[profile.{name}] resolves to panic = \"{value}\" (declared \
                                     by [profile.{by}])"
                                ),
                            ));
                        }
                    }
                }
            }
        }

        Audit {
            violations,
            runtime_workspaces,
            manifests_seen: manifests.len(),
        }
    }

    fn fix_message(manifest: &str, via: &str, problem: &str) -> String {
        format!(
            "\n{manifest}\n  \
             This workspace builds a Perry runtime archive (reachable by path: {via}),\n  \
             but {problem}.\n\n  \
             A runtime built on the unwind strategy aborts the process on any JS throw that\n  \
             crosses an extern \"C\" helper containing an interior Rust call (RFC 2945). The\n  \
             crash reads `panic in a function that cannot unwind` just below `_js_throw`,\n  \
             with a handler already armed.\n\n  \
             FIX — in {manifest}, under [profile.release] (create it if absent):\n\n      \
             [profile.release]\n      \
             panic = \"abort\"\n\n  \
             and make sure no other profile in that file re-declares `panic` as anything\n  \
             else; `inherits = \"release\"` does NOT protect you, an explicit override wins.\n  \
             Pair it with `-C force-unwind-tables=yes` (see .cargo/config.toml).\n  \
             See crates/perry/src/panic_profile_contract.rs for the full history."
        )
    }

    // ---------------------------------------------------------------- tests

    #[test]
    fn every_runtime_building_workspace_is_panic_abort() {
        let root = repo_root();
        let result = audit(&root);

        // The subject must be LIVE. A discovery regression (an over-eager
        // prune, a parse that stopped matching) would otherwise make this
        // test pass having examined nothing.
        assert!(
            result.manifests_seen >= 50,
            "the manifest walk found only {} Cargo.toml files under {} — discovery is broken, \
             so this contract is measuring nothing",
            result.manifests_seen,
            root.display()
        );
        assert!(
            result.runtime_workspaces.iter().any(|w| w == "Cargo.toml"),
            "the main workspace was not classified as runtime-building (found: {:?}) — the \
             reachability analysis is broken, so this contract is measuring nothing",
            result.runtime_workspaces
        );

        assert!(
            result.violations.is_empty(),
            "{} workspace manifest(s) would build a Perry runtime archive on the unwind panic \
             strategy:\n{}",
            result.violations.len(),
            result.violations.join("\n")
        );
    }

    /// The main workspace's shipping profiles are also pinned BY NAME, so
    /// renaming or deleting one cannot quietly shrink what the audit above
    /// judges.
    #[test]
    fn main_workspace_shipping_profiles_exist_and_abort() {
        let root = repo_root();
        let manifests = collect_manifests(&root);
        let main = manifests
            .get(&normalize(&root))
            .expect("the repo root Cargo.toml must parse");
        let profiles = profiles_of(main).expect("the repo root must declare [profile.*]");

        for name in MAIN_SHIPPING_PROFILES {
            assert!(
                profiles.contains_key(*name),
                "[profile.{name}] has disappeared from the workspace manifest; if it was \
                 renamed, update MAIN_SHIPPING_PROFILES so the rename is judged too"
            );
            match resolve_panic(Some(profiles), name) {
                Panic::Declared { value, by } => assert_eq!(
                    value, "abort",
                    "[profile.{name}] resolves to panic = \"{value}\" (declared by \
                     [profile.{by}]); see the module docs"
                ),
                Panic::CargoDefault => panic!(
                    "[profile.{name}] resolves to cargo's default panic strategy (\"unwind\"); \
                     it must resolve to \"abort\" — see the module docs"
                ),
            }
        }
    }

    /// The abort strategy is only half the contract: `panic = "abort"`
    /// omits unwind tables by default, and without them the unwinder cannot
    /// step runtime frames at all — every cross-helper throw is stranded
    /// rather than caught. The flag lives in `.cargo/config.toml` because a
    /// profile cannot carry rustflags.
    #[test]
    fn unwind_tables_are_forced_for_the_workspace() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../.cargo/config.toml");
        let cfg = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("cargo config unreadable at {path}: {e}"));
        let normalized = cfg.replace(['"', ' ', '\n'], "");
        assert!(
            normalized.contains("force-unwind-tables=yes"),
            ".cargo/config.toml must force unwind tables: panic=abort omits them, and the \
             exception transport cannot step runtime frames without them (the runtime \
             self-checks on the first `try` and aborts loudly). See the module docs."
        );
    }

    // ------------------------------------------------- sabotage (negatives)
    //
    // A contract test that cannot fail is worse than none. These build the
    // #8034 shape — a separate workspace whose members path-depend on
    // perry-runtime/perry-stdlib — and assert the audit actually reports it,
    // then that the one-line fix clears it.

    /// The provider fixture's shape, parameterized by the root's release
    /// profile body and by whether the member reaches the runtime.
    fn fixture(dir: &Path, release_body: &str, dep: &str) {
        std::fs::create_dir_all(dir.join("runtime")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            format!(
                "[workspace]\nmembers = [\"runtime\"]\nresolver = \"2\"\n\n\
                 [workspace.package]\nedition = \"2021\"\n\n\
                 [profile.release]\n{release_body}"
            ),
        )
        .unwrap();
        std::fs::write(
            dir.join("runtime/Cargo.toml"),
            format!(
                "[package]\nname = \"some-provider\"\nversion = \"0.0.0\"\n\
                 edition.workspace = true\n\n\
                 [lib]\ncrate-type = [\"staticlib\"]\n\n\
                 [dependencies]\n{dep}\n"
            ),
        )
        .unwrap();
    }

    /// A path dependency on perry-runtime under a renamed key — exactly how
    /// the #8034 provider spells it.
    const RUNTIME_DEP: &str = "perry-runtime-core = { package = \"perry-runtime\", \
                               path = \"../../../crates/perry-runtime\" }";

    #[test]
    fn sabotage_a_release_profile_with_no_panic_key_is_reported() {
        let tmp = tempfile::tempdir().unwrap();
        fixture(
            tmp.path(),
            "codegen-units = 16\nlto = false\nstrip = false\n",
            RUNTIME_DEP,
        );

        let result = audit(tmp.path());
        assert_eq!(
            result.violations.len(),
            1,
            "expected exactly one violation, got: {:?}",
            result.violations
        );
        let msg = &result.violations[0];
        assert!(
            msg.contains("Cargo.toml") && msg.contains("declares no `panic` key"),
            "the message must name the file and the cause: {msg}"
        );
        assert!(
            msg.contains("panic = \"abort\""),
            "the message must state the fix: {msg}"
        );
        assert!(
            msg.contains("perry-runtime"),
            "the message must name the witness that put this workspace in scope: {msg}"
        );
    }

    #[test]
    fn sabotage_an_explicit_unwind_is_reported() {
        let tmp = tempfile::tempdir().unwrap();
        fixture(tmp.path(), "panic = \"unwind\"\n", RUNTIME_DEP);

        let result = audit(tmp.path());
        assert_eq!(
            result.violations.len(),
            1,
            "expected exactly one violation, got: {:?}",
            result.violations
        );
        assert!(result.violations[0].contains("panic = \"unwind\""));
    }

    /// Instance 2's shape: a second profile that `inherits` a correct
    /// `release` and then overrides it.
    #[test]
    fn sabotage_an_inheriting_profile_that_overrides_is_reported() {
        let tmp = tempfile::tempdir().unwrap();
        fixture(
            tmp.path(),
            "panic = \"abort\"\n\n[profile.dist]\ninherits = \"release\"\npanic = \"unwind\"\n",
            RUNTIME_DEP,
        );

        let result = audit(tmp.path());
        assert_eq!(
            result.violations.len(),
            1,
            "expected exactly one violation, got: {:?}",
            result.violations
        );
        assert!(
            result.violations[0].contains("[profile.dist]"),
            "{:?}",
            result.violations
        );
    }

    #[test]
    fn the_one_line_fix_clears_the_sabotage() {
        let tmp = tempfile::tempdir().unwrap();
        fixture(
            tmp.path(),
            "codegen-units = 16\nlto = false\nstrip = false\npanic = \"abort\"\n",
            RUNTIME_DEP,
        );

        let result = audit(tmp.path());
        assert!(
            result.violations.is_empty(),
            "the fixed manifest must pass: {:?}",
            result.violations
        );
        assert_eq!(
            result.runtime_workspaces.len(),
            1,
            "and it must still have been examined, not merely skipped"
        );
    }

    /// A profile that only `inherits = "release"` needs no `panic` line of
    /// its own — that is what `perry-dev` does in the main workspace.
    #[test]
    fn inheriting_a_correct_release_without_re_declaring_passes() {
        let tmp = tempfile::tempdir().unwrap();
        fixture(
            tmp.path(),
            "panic = \"abort\"\n\n[profile.perry-dev]\ninherits = \"release\"\nopt-level = 1\n",
            RUNTIME_DEP,
        );

        let result = audit(tmp.path());
        assert!(result.violations.is_empty(), "{:?}", result.violations);
    }

    /// The false-alarm guard. `benchmarks/json_polyglot` is a real
    /// standalone workspace with a `[profile.release]` that has no `panic`
    /// key — and it must stay silent, because it cannot reach the runtime.
    /// A rule that flags every workspace would be muted within a week.
    #[test]
    fn a_workspace_that_cannot_reach_the_runtime_is_not_flagged() {
        let tmp = tempfile::tempdir().unwrap();
        fixture(
            tmp.path(),
            "opt-level = 3\nlto = false\ncodegen-units = 16\n",
            "serde_json = \"1\"",
        );

        let result = audit(tmp.path());
        assert!(
            result.violations.is_empty(),
            "a workspace with no path to perry-runtime must not be flagged: {:?}",
            result.violations
        );
        assert!(result.runtime_workspaces.is_empty());
    }

    /// `[profile.*]` in a non-root member is ignored by cargo, so the audit
    /// must not judge it — nor let it excuse a bad root.
    #[test]
    fn a_profile_in_a_non_root_member_is_neither_trusted_nor_blamed() {
        let tmp = tempfile::tempdir().unwrap();
        fixture(
            tmp.path(),
            "codegen-units = 16\n", // root: missing panic -> must fail
            RUNTIME_DEP,
        );
        // The member "fixes" it locally. Cargo ignores this entirely.
        let member = tmp.path().join("runtime/Cargo.toml");
        let text = std::fs::read_to_string(&member).unwrap();
        std::fs::write(
            &member,
            format!("{text}\n[profile.release]\npanic = \"abort\"\n"),
        )
        .unwrap();

        let result = audit(tmp.path());
        assert_eq!(
            result.violations.len(),
            1,
            "a member-level profile must not excuse the root: {:?}",
            result.violations
        );
        assert!(
            result.violations[0].contains("\nCargo.toml\n"),
            "the ROOT must be named"
        );
    }

    /// A dev-dependency is linked only into test/bench harnesses, where
    /// cargo ignores `panic` outright — such a workspace ships no archive.
    #[test]
    fn a_dev_dependency_on_the_runtime_is_not_an_edge() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("runtime")).unwrap();
        std::fs::write(
            tmp.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"runtime\"]\n\n[profile.release]\ncodegen-units = 16\n",
        )
        .unwrap();
        std::fs::write(
            tmp.path().join("runtime/Cargo.toml"),
            format!(
                "[package]\nname = \"harness\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n\
                 [dev-dependencies]\n{RUNTIME_DEP}\n"
            ),
        )
        .unwrap();

        let result = audit(tmp.path());
        assert!(result.violations.is_empty(), "{:?}", result.violations);
    }

    /// Reachability is transitive through intermediate path dependencies —
    /// a provider that only names `perry-ext-http` still ends up linking the
    /// runtime.
    #[test]
    fn reachability_is_transitive_through_path_dependencies() {
        let tmp = tempfile::tempdir().unwrap();
        fixture(
            tmp.path(),
            "codegen-units = 16\n",
            "middle = { path = \"../middle\" }",
        );
        std::fs::create_dir_all(tmp.path().join("middle")).unwrap();
        std::fs::write(
            tmp.path().join("middle/Cargo.toml"),
            "[package]\nname = \"middle\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n\
             [dependencies]\nperry-stdlib = { path = \"../../elsewhere/perry-stdlib\" }\n",
        )
        .unwrap();

        let result = audit(tmp.path());
        assert_eq!(result.violations.len(), 1, "{:?}", result.violations);
        assert!(
            result.violations[0].contains("perry-stdlib"),
            "{:?}",
            result.violations
        );
    }
}
