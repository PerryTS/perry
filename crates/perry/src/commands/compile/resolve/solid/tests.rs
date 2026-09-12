use super::*;

fn write(dir: &Path, name: &str, source: &str) {
    let path = dir.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, source).unwrap();
}

fn solid_package(dir: &Path) {
    write(
        dir,
        "package.json",
        r#"{"name":"solid-js","exports":{
        ".":{"node":"./dist/server.js","default":"./dist/solid.js"},
        "./store":{"node":"./store/dist/server.js","default":"./store/dist/store.js"}
    }}"#,
    );
    for name in [
        "dist/server.js",
        "dist/solid.js",
        "store/dist/server.js",
        "store/dist/store.js",
    ] {
        write(dir, name, "export const marker = 1;");
    }
}

#[test]
fn client_swap_is_graph_wide_and_preserves_package_instances() {
    let dir = tempfile::tempdir().unwrap();
    let mut ctx = CompilationContext::new(dir.path().to_path_buf());
    ctx.compile_packages.insert("solid-js".into());
    let top = dir.path().join("node_modules/solid-js");
    let nested = dir.path().join("node_modules/holder/node_modules/solid-js");
    solid_package(&top);
    solid_package(&nested);
    for (importer, package) in [
        (dir.path().join("main.tsx"), top),
        (dir.path().join("node_modules/holder/index.js"), nested),
    ] {
        ctx.solid_client = false;
        let server = resolve_import_with_context("solid-js", &importer, &ctx)
            .unwrap()
            .0;
        assert_eq!(
            server,
            package.join("dist/server.js").canonicalize().unwrap()
        );
        ctx.solid_client = true;
        for (source, expected) in [
            ("solid-js", "dist/solid.js"),
            ("solid-js/store", "store/dist/store.js"),
        ] {
            let (path, kind) = resolve_import_with_context(source, &importer, &ctx).unwrap();
            assert_eq!(path, package.join(expected).canonicalize().unwrap());
            assert_eq!(kind, ModuleKind::NativeCompiled);
        }
    }
    let unrelated = dir.path().join("node_modules/unrelated");
    write(&unrelated, "package.json", r#"{"name":"unrelated"}"#);
    write(&unrelated, "dist/server.js", "");
    write(&unrelated, "dist/solid.js", "");
    assert!(client_entry(&unrelated.join("dist/server.js")).is_none());
}

#[test]
fn opentui_uses_bun_only_when_the_bun_platform_is_selected() {
    let dir = tempfile::tempdir().unwrap();
    let package = dir.path().join("node_modules/@opentui/solid");
    write(
        &package,
        "package.json",
        r#"{"name":"@opentui/solid","exports":{
        ".":{"bun":"./index.bun.js","node":"./index.js","default":"./index.js"}
    }}"#,
    );
    write(&package, "index.bun.js", "export const marker = 'bun';");
    write(&package, "index.js", "export const marker = 'node';");
    let mut ctx = CompilationContext::new(dir.path().to_path_buf());
    ctx.compile_packages.insert("@opentui/solid".into());
    for (bun, expected) in [(false, "index.js"), (true, "index.bun.js")] {
        ctx.bun_platform = bun;
        let path =
            resolve_import_with_context("@opentui/solid", &dir.path().join("main.tsx"), &ctx)
                .unwrap()
                .0;
        assert_eq!(path, package.join(expected).canonicalize().unwrap());
    }
}
