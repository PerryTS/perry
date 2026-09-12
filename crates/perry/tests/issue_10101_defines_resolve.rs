use std::path::Path;
use std::process::{Command, Output};

fn compile(root: &Path, args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_perry"));
    // LLVM's statepoint pass does not support Windows catchpad EH (#7354).
    if cfg!(windows) {
        command.env("PERRY_RS4GC", "0");
    }
    command
        .current_dir(root)
        .args(["compile", "main.ts", "-o", "app.exe"])
        .args(args)
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env_remove("PERRY_NO_CACHE")
        .env_remove("PERRY_DISABLE_BUILD_CACHE")
        .output()
        .expect("compile")
}

fn success(output: &Output) -> String {
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n")
}

fn run(root: &Path, args: &[&str]) -> String {
    success(&compile(root, args));
    success(
        &Command::new(root.join("app.exe"))
            .current_dir(root)
            .output()
            .unwrap(),
    )
}

#[test]
fn defines_fold_guards_and_invalidate_both_caches() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("main.ts"),
        r#"
declare const OPENCODE_VERSION: string;
console.log(typeof OPENCODE_VERSION === "string" ? OPENCODE_VERSION : "local");
console.log(typeof OPENCODE_MODELS_DEV === "undefined" ? "fetch" : OPENCODE_MODELS_DEV.model);
function local(OPENCODE_VERSION: string) { return OPENCODE_VERSION; }
console.log(local("shadow"));
"#,
    )
    .unwrap();
    assert_eq!(run(root, &[]), "local\nfetch\nshadow\n");
    let args = [
        "--define",
        "OPENCODE_VERSION=\"1.18.30\"",
        "--define",
        "OPENCODE_MODELS_DEV={\"model\":\"snapshot\"}",
    ];
    assert_eq!(run(root, &args), "1.18.30\nsnapshot\nshadow\n");
    // Same output path exercises the whole-build probe as well as object reuse.
    assert_eq!(run(root, &args), "1.18.30\nsnapshot\nshadow\n");
    assert_eq!(
        run(root, &["--define", "OPENCODE_VERSION=\"1.18.31\""]),
        "1.18.31\nfetch\nshadow\n"
    );
    assert_eq!(run(root, &[]), "local\nfetch\nshadow\n");
}

#[test]
fn json_expressions_and_cli_override_legacy_package_literals() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("main.ts"),
        "console.log(process.env.FLAVOR, VERSION, COUNT, ALIAS);",
    )
    .unwrap();
    std::fs::write(root.join("package.json"), r#"{"perry":{"define":{"process.env.FLAVOR":"legacy","VERSION":"legacy","COUNT":1,"ALIAS":0}}}"#).unwrap();
    assert_eq!(run(root, &[]), "legacy legacy 1 0\n");
    std::fs::write(root.join("perry.json"), r#"{"define":{"process.env.FLAVOR":"'config'","VERSION":"'json'","COUNT":"2","ALIAS":"Math.PI"}}"#).unwrap();
    assert!(run(
        root,
        &["--define", "VERSION='first'", "--define", "VERSION='cli'"]
    )
    .starts_with("config cli 2 3.14159"));
    std::fs::write(
        root.join("perry.json"),
        r#"{"define":{"VERSION":"'changed'"}}"#,
    )
    .unwrap();
    assert_eq!(run(root, &[]), "legacy changed 1 0\n");
}

#[test]
fn invalid_defines_are_diagnosed() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.ts"), "console.log('ok');").unwrap();
    for value in ["MISSING_EQUALS", "BAD-NAME=1", "VALUE=run()", "VALUE="] {
        let output = compile(dir.path(), &["--define", value, "--no-link"]);
        assert!(!output.status.success(), "accepted {value}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("define"));
    }
}

#[test]
fn resolves_static_assets_and_runtime_installed_packages_with_directory_or_url_parent() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let package = root.join("node_modules/@fixture/core");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(package.join("package.json"), r#"{"exports":{"./parser.worker":"./parser.worker.js","./tree-sitter.wasm":"./tree-sitter.wasm"}}"#).unwrap();
    std::fs::write(
        package.join("parser.worker.js"),
        "throw new Error('resolve must not execute');",
    )
    .unwrap();
    std::fs::write(package.join("tree-sitter.wasm"), b"\0asm").unwrap();
    std::fs::write(
        root.join("main.ts"),
        r#"
console.log(import.meta.resolve("@fixture/core/parser.worker").endsWith("/parser.worker.js"));
console.log(import.meta.resolve("@fixture/core/tree-sitter.wasm").endsWith("/tree-sitter.wasm"));
const name = process.argv[2];
const parent = process.argv[3];
const resolved = import.meta.resolve(name, parent);
console.log(resolved.startsWith("file://"), resolved.endsWith("/entry%20space.js"));
const resolve = import.meta.resolve;
console.log(resolve(name, parent) === resolved);
console.log(resolve(process.argv[4]).endsWith("/default.js"));
try { resolve("missing-package", parent); } catch (error) { console.log(error.code); }
console.log(import.meta.resolve("node:fs"));
"#,
    )
    .unwrap();
    success(&compile(root, &[]));
    // Literal resolutions have become URLs in the executable, independent of
    // the runtime package resolver. Resolving them did not execute their bodies.
    std::fs::remove_dir_all(&package).unwrap();
    // This package exists only AFTER compilation, proving disk resolution is dynamic.
    let parent = root.join("installed");
    let installed = parent.join("node_modules/x");
    std::fs::create_dir_all(&installed).unwrap();
    std::fs::write(
        installed.join("package.json"),
        r#"{"exports":{".":{"import":"./entry space.js","require":"./wrong.cjs"}}}"#,
    )
    .unwrap();
    std::fs::write(
        installed.join("entry space.js"),
        "throw new Error('do not execute');",
    )
    .unwrap();
    let default_package = root.join("node_modules/default-fixture");
    std::fs::create_dir_all(&default_package).unwrap();
    std::fs::write(
        default_package.join("package.json"),
        r#"{"main":"default.js"}"#,
    )
    .unwrap();
    std::fs::write(
        default_package.join("default.js"),
        "throw new Error('do not execute');",
    )
    .unwrap();
    for parent_arg in [
        parent.to_string_lossy().to_string(),
        url::Url::from_directory_path(&parent).unwrap().to_string(),
        url::Url::from_file_path(parent.join("caller.ts"))
            .unwrap()
            .to_string(),
    ] {
        let output = Command::new(root.join("app.exe"))
            .args(["x", &parent_arg, "default-fixture"])
            .output()
            .unwrap();
        assert_eq!(
            success(&output),
            "true\ntrue\ntrue true\ntrue\ntrue\nERR_MODULE_NOT_FOUND\nnode:fs\n"
        );
    }
}

#[test]
fn parent_url_and_defined_worker_entries_are_discovered() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("cli")).unwrap();
    std::fs::create_dir_all(root.join("tui")).unwrap();
    std::fs::write(root.join("tui/worker.ts"), "postMessage('worker-ready');").unwrap();
    std::fs::write(root.join("main.ts"), "import './cli/main';").unwrap();
    std::fs::write(
        root.join("cli/main.ts"),
        r#"
setTimeout(() => process.exit(2), 5000);
const worker = new Worker(new URL("../tui/worker.ts", import.meta.url));
worker.onmessage = (event: any) => { console.log(event.data); process.exit(0); };
"#,
    )
    .unwrap();
    let discovered = success(&compile(root, &["--no-link"]));
    assert!(
        discovered.contains("3 native, 0 JavaScript"),
        "{discovered}"
    );
    std::fs::write(root.join("cli/main.ts"), r#"
setTimeout(() => process.exit(2), 5000);
const path = typeof OPENCODE_WORKER_PATH === "undefined" ? new URL("../tui/worker.ts", import.meta.url) : OPENCODE_WORKER_PATH;
const worker = new Worker(path);
worker.onmessage = (event: any) => { console.log(event.data); process.exit(0); };
"#).unwrap();
    let define = format!(
        "OPENCODE_WORKER_PATH={}",
        serde_json::to_string(&root.join("tui/worker.ts").to_string_lossy()).unwrap()
    );
    let discovered = success(&compile(root, &["--no-link", "--define", &define]));
    assert!(
        discovered.contains("3 native, 0 JavaScript"),
        "{discovered}"
    );
}
