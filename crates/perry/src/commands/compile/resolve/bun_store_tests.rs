use super::*;

/// Issue #5914 — bun's flat/isolated linker layout keeps purely-transitive
/// dependencies only inside `node_modules/.bun/<pkg>@<version>/node_modules/<pkg>`,
/// with no top-level `node_modules/<pkg>` symlink. `enumerate_installed_packages`
/// must find these too, not just the top-level entries, so the
/// `compilePackages: ["*"]` wildcard actually covers a real bun install.
#[test]
fn enumerate_installed_packages_finds_bun_flat_store_only_packages() {
    let dir = tempfile::tempdir().expect("tempdir");
    let nm = dir.path().join("node_modules");

    // A normal top-level (hoisted) package.
    std::fs::create_dir_all(nm.join("hoisted-pkg")).unwrap();

    // A bun-flat-store-only package: no top-level symlink, only reachable
    // via `.bun/<pkg>@<version>/node_modules/<pkg>`.
    std::fs::create_dir_all(nm.join(".bun/ajv@8.20.0/node_modules/ajv")).unwrap();

    // Scoped variant: `.bun/@scope+pkg@<version>/node_modules/@scope/pkg`.
    std::fs::create_dir_all(
        nm.join(".bun/@opentelemetry+core@2.6.1/node_modules/@opentelemetry/core"),
    )
    .unwrap();

    let found = enumerate_installed_packages(dir.path());
    assert!(found.contains("hoisted-pkg"));
    assert!(
        found.contains("ajv"),
        "expected bun-flat-store-only package 'ajv' to be found, got: {found:?}"
    );
    assert!(
        found.contains("@opentelemetry/core"),
        "expected scoped bun-flat-store-only package to be found, got: {found:?}"
    );
}

/// Issue #5914 followup — in a bun workspace, `.bun` typically lives ONLY at
/// the true monorepo root, while `find_node_modules` stops at the *nearest*
/// ancestor `node_modules` (a workspace member's own, bun-created, `.bun`-less
/// `node_modules` for its first-party sibling symlinks). A `project_root`
/// pointing at that workspace member must still find root-level bun-only
/// transitive deps.
#[test]
fn enumerate_installed_packages_finds_bun_store_from_workspace_member_root() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    // Root node_modules: only the bun store, no top-level symlinks.
    std::fs::create_dir_all(root.join("node_modules/.bun/ajv@8.20.0/node_modules/ajv")).unwrap();

    // Workspace member with its OWN nearer node_modules (first-party sibling
    // symlink only, no .bun subdir) — this is what `find_node_modules` finds
    // first starting from the member's directory.
    let member = root.join("packages/app");
    std::fs::create_dir_all(member.join("node_modules/@myorg/core")).unwrap();

    let found = enumerate_installed_packages(&member);
    assert!(
        found.contains("@myorg/core"),
        "expected the member's own nearest node_modules entry to be found, got: {found:?}"
    );
    assert!(
        found.contains("ajv"),
        "expected the ROOT's bun-store-only package to be found from a workspace-member \
         project_root, got: {found:?}"
    );
}
