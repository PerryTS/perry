//! Which packages are installed under a project: the node_modules, bun and pnpm store layouts.

use super::*;

/// Enumerate every installed package name reachable from `project_root`'s
/// `node_modules` tree (the nearest one walking up, plus any nested
/// `node_modules` directories for transitive deps npm chose not to hoist).
///
/// Used by #3527 to materialize the `"*"` / `"@scope/*"` wildcard entries in
/// `perry.compilePackages` into concrete package names. The routing
/// predicates (`is_in_compile_package`, the HIR `COMPILE_PACKAGES_OVERRIDE`,
/// and the many `compile_packages.contains(name)` sites) all match exact
/// package names, so the wildcard has to be expanded before routing or it is a
/// silent no-op. Returns scoped names as `@scope/name`.
#[cfg(test)]
pub(in crate::commands::compile) fn enumerate_installed_packages(
    project_root: &Path,
) -> HashSet<String> {
    enumerate_installed_package_roots(project_root)
        .into_keys()
        .collect()
}

/// Enumerate installed package names together with every directory that
/// supplied that name. Wildcard compile routing normally needs only the names,
/// but policy checks such as the Node native-addon guard must inspect package
/// roots before deciding whether the wildcard may select them.
pub(in crate::commands::compile) fn enumerate_installed_package_roots(
    project_root: &Path,
) -> HashMap<String, Vec<PathBuf>> {
    let mut out = HashMap::new();
    // #5914: bun's "flat"/isolated linker layout keeps non-hoisted transitive
    // dependencies solely inside `node_modules/.bun/<pkg>@<version>/node_modules/<pkg>`
    // (and the scoped `@scope+pkg@<version>` variant), with no corresponding
    // top-level `node_modules/<pkg>` symlink — bun only symlinks packages
    // that are direct dependencies of some workspace package into the top
    // level. `collect_packages_in_node_modules` correctly skips `.bun` as a
    // dotdir (it is bun's internal store, not itself a package), which means
    // those transitive-only packages are invisible to the `"*"` /
    // `"@scope/*"` wildcard expansion above.
    //
    // Walk every ancestor in Node resolution order. A package-local cache or
    // a workspace member's node_modules must not hide packages further up.
    // One `visited` set spans the whole enumeration. A `node_modules` reached
    // twice holds the same packages both times, so skipping the repeat only
    // drops duplicates the dedupe below would discard anyway.
    let mut visited = HashSet::new();
    for nm in ancestor_node_modules_dirs(project_root) {
        collect_packages_in_node_modules(&nm, &mut out, &mut visited);
        collect_packages_in_bun_store(&nm, &mut out, &mut visited);
        collect_packages_in_pnpm_store(&nm, &mut out, &mut visited);
    }
    // Symlinks and recursively discovered nested packages can expose the same
    // physical root more than once. Keep its first occurrence, preserving the
    // nearest-first order rather than sorting paths lexically.
    for roots in out.values_mut() {
        let mut seen = HashSet::new();
        roots.retain_mut(|root| {
            *root = fs::canonicalize(&*root).unwrap_or_else(|_| root.clone());
            seen.insert(root.clone())
        });
    }
    out
}

/// See the `.bun` note on `enumerate_installed_packages`. Each
/// `.bun/<entry>/node_modules` subdirectory has the exact same shape as an
/// ordinary `node_modules` directory, so walk it with the same collector.
/// `node_modules` need not exist (`fs::read_dir` on the derived `.bun` path
/// simply fails and returns).
pub(super) fn collect_packages_in_bun_store(
    node_modules: &Path,
    out: &mut HashMap<String, Vec<PathBuf>>,
    visited: &mut HashSet<PathBuf>,
) {
    let bun_dir = node_modules.join(".bun");
    let entries = match fs::read_dir(&bun_dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let nested = entry.path().join("node_modules");
        if nested.is_dir() {
            collect_packages_in_node_modules(&nested, out, visited);
        }
    }
}

/// pnpm's isolated layout links only a workspace package's direct
/// dependencies into its `node_modules`; transitive ones are reachable through
/// `node_modules/.pnpm`, either its hidden-hoist `node_modules` (the only
/// entry when pnpm links packages from a global virtual store) or
/// `.pnpm/<pkg>@<version>/node_modules`. Without them the
/// wildcard compiles a package from TypeScript source while its transitive
/// dependency stays on published JavaScript, which lacks source-only exports
/// such as a `const enum` (`tldts` importing `FLAG` from `tldts-core`).
pub(super) fn collect_packages_in_pnpm_store(
    node_modules: &Path,
    out: &mut HashMap<String, Vec<PathBuf>>,
    visited: &mut HashSet<PathBuf>,
) {
    let pnpm_dir = node_modules.join(".pnpm");
    let entries = match fs::read_dir(&pnpm_dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    collect_packages_in_node_modules(&pnpm_dir.join("node_modules"), out, visited);
    for entry in entries.flatten() {
        let nested = entry.path().join("node_modules");
        if nested.is_dir() {
            collect_packages_in_node_modules(&nested, out, visited);
        }
    }
}

/// Walk a single `node_modules` directory, recording each package name and
/// recursing into any nested `node_modules` it contains.
///
/// `visited` holds the canonical path of every directory already walked, and
/// is what makes the recursion terminate. Workspace layouts symlink members
/// into each other's `node_modules`, so `@scope/a/node_modules/@scope/a/...`
/// is an infinite directory tree, and every extra outlet per node multiplies
/// the paths explored. A cycle whose nodes have one outlet each only stops
/// because the accumulating path hits `ENAMETOOLONG`; with two or more it does
/// not stop at all. Measured before this guard: 73k `openat` calls in 25s for
/// two packages, and a 19-package workspace exhausted 32GB without collecting
/// a single module (#12822).
pub(super) fn collect_packages_in_node_modules(
    node_modules: &Path,
    out: &mut HashMap<String, Vec<PathBuf>>,
    visited: &mut HashSet<PathBuf>,
) {
    // Canonicalize so the same physical directory reached by different
    // symlink paths counts as one visit. An un-canonicalizable path (a broken
    // link, a race with an installer) is walked under its literal path rather
    // than skipped.
    let key = fs::canonicalize(node_modules).unwrap_or_else(|_| node_modules.to_path_buf());
    if !visited.insert(key) {
        return;
    }
    let entries = match fs::read_dir(node_modules) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        // Skip npm bookkeeping dirs (`.bin`, `.cache`, `.package-lock.json`).
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if name.starts_with('@') {
            // Scope directory: each child is a `@scope/pkg`.
            if let Ok(scoped) = fs::read_dir(&path) {
                for sub in scoped.flatten() {
                    let sub_name = sub.file_name();
                    let sub_name = sub_name.to_string_lossy();
                    if sub_name.starts_with('.') {
                        continue;
                    }
                    let sub_path = sub.path();
                    if !sub_path.is_dir() {
                        continue;
                    }
                    out.entry(format!("{}/{}", name, sub_name))
                        .or_default()
                        .push(sub_path.clone());
                    let nested = sub_path.join("node_modules");
                    if nested.is_dir() {
                        collect_packages_in_node_modules(&nested, out, visited);
                    }
                }
            }
            continue;
        }
        out.entry(name.to_string()).or_default().push(path.clone());
        let nested = path.join("node_modules");
        if nested.is_dir() {
            collect_packages_in_node_modules(&nested, out, visited);
        }
    }
}
