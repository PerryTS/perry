use super::*;

#[test]
fn bookkeeping_node_modules_does_not_hide_ancestor_packages() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path();
    let pkg = root.join("node_modules/demo");
    fs::create_dir_all(&pkg).unwrap();
    fs::write(
        pkg.join("package.json"),
        r#"{"name":"demo","type":"module"}"#,
    )
    .unwrap();
    fs::write(pkg.join("index.js"), "console.log('hello');").unwrap();
    // A compile writes this cache under the entry package. The next compile
    // must still enumerate demo and classify index.js as a compiled package.
    let before = enumerate_installed_package_roots(&pkg);
    fs::create_dir_all(pkg.join("node_modules/.cache/perry")).unwrap();
    let after = enumerate_installed_package_roots(&pkg);
    assert_eq!(before.get("demo"), after.get("demo"));
    assert_eq!(after["demo"], vec![pkg.canonicalize().unwrap()]);
}

#[test]
fn ancestor_package_roots_keep_nearest_first_order() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path();
    // Lexical sorting puts the far copy first, which violates Node priority.
    let far = root.join("node_modules/dup");
    let near = root.join("z/member/node_modules/dup");
    fs::create_dir_all(&far).unwrap();
    fs::create_dir_all(&near).unwrap();
    let roots = enumerate_installed_package_roots(&root.join("z/member"));
    assert_eq!(
        roots["dup"],
        vec![near.canonicalize().unwrap(), far.canonicalize().unwrap()]
    );
}

#[test]
fn bun_store_roots_remain_visible_with_bookkeeping_nearby() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path();
    let pkg = root.join("node_modules/.bun/transitive@1/node_modules/transitive");
    fs::create_dir_all(&pkg).unwrap();
    let member = root.join("member");
    fs::create_dir_all(member.join("node_modules/.cache")).unwrap();
    let roots = enumerate_installed_package_roots(&member);
    assert_eq!(roots["transitive"], vec![pkg.canonicalize().unwrap()]);
}

#[cfg(unix)]
#[test]
fn ancestor_package_roots_deduplicate_physical_paths() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path();
    let physical = root.join("store/dup");
    fs::create_dir_all(&physical).unwrap();
    fs::create_dir_all(root.join("node_modules")).unwrap();
    fs::create_dir_all(root.join("z/member/node_modules")).unwrap();
    std::os::unix::fs::symlink(&physical, root.join("node_modules/dup")).unwrap();
    std::os::unix::fs::symlink(&physical, root.join("z/member/node_modules/dup")).unwrap();
    let roots = enumerate_installed_package_roots(&root.join("z/member"));
    assert_eq!(roots["dup"], vec![physical.canonicalize().unwrap()]);
}
