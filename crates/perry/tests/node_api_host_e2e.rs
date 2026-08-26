//! End-to-end gate for #8523's Node-API host.
//!
//! This builds a real `.node` shared library, compiles a Perry executable that
//! imports it, proves the addon called symbols exported by that executable,
//! and then corrupts the shipped payload to prove runtime authentication is
//! live. The fixture uses direct Node-API C calls so no Node/V8 runtime can
//! accidentally make the test green.

#![cfg(any(unix, windows))]

use object::Object;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const ADDON_C: &str = include_str!("fixtures/node_api_host/addon.c");
#[cfg(windows)]
const ADDON_DEF: &str = include_str!("fixtures/node_api_host/addon.def");
const HOST_SYMBOLS: &str = include_str!("../../perry-runtime/src/node_api_host/symbols.txt");

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonical Perry workspace")
}

fn command_available(name: &str) -> bool {
    Command::new(name).arg("--version").output().is_ok()
}

fn require_tool(name: &str) -> bool {
    if command_available(name) {
        return true;
    }
    if std::env::var_os("CI").is_some() || std::env::var_os("PERRY_REQUIRE_NODE_API_E2E").is_some()
    {
        panic!("required Node-API e2e tool `{name}` is unavailable");
    }
    eprintln!("SKIP: `{name}` is unavailable; set PERRY_REQUIRE_NODE_API_E2E=1 to require it");
    false
}

fn run(mut command: Command, subject: &str) -> Output {
    let output = command.output().unwrap_or_else(|error| {
        panic!("could not run {subject}: {error}");
    });
    assert!(
        output.status.success(),
        "{subject} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn compile_addon(root: &Path, package: &Path) {
    let source = root.join("addon.c");
    std::fs::write(&source, ADDON_C).expect("write addon C fixture");
    let addon = package.join("addon.node");

    #[cfg(windows)]
    {
        let definition = root.join("addon.def");
        let import_library = root.join("app.lib");
        std::fs::write(&definition, ADDON_DEF).expect("write addon export definition");
        let machine = match std::env::consts::ARCH {
            "x86_64" => "i386:x86-64",
            "aarch64" => "arm64",
            arch => panic!("unsupported Windows Node-API fixture architecture `{arch}`"),
        };
        let mut dlltool = Command::new("llvm-dlltool");
        dlltool
            .current_dir(root)
            .arg("-m")
            .arg(machine)
            .arg("-d")
            .arg(&definition)
            .arg("-l")
            .arg(&import_library);
        run(dlltool, "llvm-dlltool import-library generation");

        let mut clang = Command::new("clang");
        clang
            .current_dir(root)
            .arg("-shared")
            .arg("-o")
            .arg(&addon)
            .arg(&source)
            .arg(&import_library);
        run(clang, "Windows Node-API fixture build");
    }

    #[cfg(unix)]
    {
        let mut clang = Command::new("clang");
        clang.current_dir(root).arg("-shared");
        #[cfg(not(target_os = "macos"))]
        clang.arg("-fPIC");
        #[cfg(target_os = "macos")]
        clang.args(["-undefined", "dynamic_lookup"]);
        clang.arg("-o").arg(&addon).arg(&source);
        run(clang, "Unix Node-API fixture build");
    }

    assert!(addon.is_file(), "fixture addon was not produced");
}

fn compile_app(root: &Path, entry: &Path, output: &Path) -> Output {
    let mut command = Command::new(perry_bin());
    command
        .current_dir(root)
        .env("PERRY_WORKSPACE_ROOT", workspace_root())
        .arg("compile")
        .arg(entry)
        .arg("-o")
        .arg(output)
        .arg("--no-cache");
    if std::env::var_os("PERRY_E2E_VERBOSE").is_some() {
        command.arg("-vv");
    }
    command.output().expect("run perry compile")
}

fn find_node_file(path: &Path) -> Option<PathBuf> {
    let mut entries = std::fs::read_dir(path)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    entries.sort();
    for path in entries {
        if path.extension().and_then(|extension| extension.to_str()) == Some("node") {
            return Some(path);
        }
        if path.is_dir() {
            if let Some(found) = find_node_file(&path) {
                return Some(found);
            }
        }
    }
    None
}

fn npm_e2e_required() -> bool {
    std::env::var_os("CI").is_some()
        || std::env::var("PERRY_REQUIRE_NPM_E2E").ok().as_deref() == Some("1")
}

fn install_pinned_package(root: &Path, package: &str, subject: &str) -> bool {
    let required = npm_e2e_required();
    if !command_available("npm") {
        if required {
            panic!("npm is required for the {subject} gate");
        }
        eprintln!("SKIP: npm is unavailable; set PERRY_REQUIRE_NPM_E2E=1 to require {subject}");
        return false;
    }

    let install = Command::new("npm")
        .current_dir(root)
        .args([
            "install",
            "--ignore-scripts",
            "--no-audit",
            "--no-fund",
            package,
        ])
        .output();
    match install {
        Ok(output) if output.status.success() => true,
        Ok(output) if required => {
            panic!(
                "npm install for {subject} failed\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        Ok(output) => {
            eprintln!(
                "SKIP: npm install for {subject} failed (offline?)\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            false
        }
        Err(error) if required => panic!("could not run npm for {subject}: {error}"),
        Err(error) => {
            eprintln!("SKIP: could not run npm for {subject}: {error}");
            false
        }
    }
}

#[test]
fn real_node_api_addon_resolves_from_host_and_authenticates_sidecar() {
    if !require_tool("clang") {
        return;
    }
    #[cfg(windows)]
    if !require_tool("llvm-dlltool") {
        return;
    }

    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let package = root.join("node_modules/fixture-addon");
    std::fs::create_dir_all(&package).expect("create fixture package");
    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "perry-node-api-e2e",
  "private": true,
  "perry": {
    "compilePackages": ["fixture-addon"],
    "allow": { "compilePackages": ["fixture-addon"] },
    "nativeAddons": ["fixture-addon"]
  }
}"#,
    )
    .expect("write project manifest");
    std::fs::write(
        package.join("package.json"),
        r#"{"name":"fixture-addon","version":"1.0.0","main":"index.js"}"#,
    )
    .expect("write addon manifest");
    std::fs::write(
        package.join("index.js"),
        "module.exports = require(\"./addon.node\")\n",
    )
    .expect("write addon wrapper");
    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        r#"const addon = require("fixture-addon")
console.log("node-api-answer", addon.answer)
console.log("node-api-add", addon.add(19, 23))
const direct = { exports: {} }
process.dlopen(direct, "fixture-addon/addon.node")
console.log("node-api-cache", direct.exports === addon)
"#,
    )
    .expect("write Perry entry");
    compile_addon(root, &package);

    let executable = root.join(if cfg!(windows) { "app.exe" } else { "app" });
    let compile = compile_app(root, &entry, &executable);
    assert!(
        compile.status.success(),
        "Perry Node-API compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    assert!(executable.is_file(), "host executable was not produced");

    let host_bytes = std::fs::read(&executable).expect("read host executable");
    let host = object::File::parse(&*host_bytes).expect("parse host executable");
    let exported = host
        .exports()
        .expect("read host exports")
        .into_iter()
        .map(|export| {
            String::from_utf8_lossy(export.name())
                .trim_start_matches('_')
                .to_string()
        })
        .collect::<BTreeSet<_>>();
    let inventory = HOST_SYMBOLS
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty() && !name.starts_with('#'))
        .collect::<BTreeSet<_>>();
    let missing = inventory
        .iter()
        .filter(|name| !exported.contains(**name))
        .copied()
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "host omitted Node-API exports required by its inventory: {missing:?}"
    );

    let control_entry = root.join("control.ts");
    std::fs::write(
        &control_entry,
        r#"const addon = { answer: 8523, add: (left: number, right: number) => left + right }
console.log("node-api-answer", addon.answer)
console.log("node-api-add", addon.add(19, 23))
const direct = { exports: addon }
console.log("node-api-cache", direct.exports === addon)
"#,
    )
    .expect("write no-addon size control");
    let control_executable = root.join(if cfg!(windows) {
        "control.exe"
    } else {
        "control"
    });
    let control_compile = compile_app(root, &control_entry, &control_executable);
    assert!(
        control_compile.status.success(),
        "no-addon control compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&control_compile.stdout),
        String::from_utf8_lossy(&control_compile.stderr)
    );
    let control_bytes = std::fs::read(&control_executable).expect("read no-addon control");
    let control = object::File::parse(&*control_bytes).expect("parse no-addon control");
    let control_node_exports = control
        .exports()
        .expect("read no-addon exports")
        .into_iter()
        .filter(|export| {
            let name = String::from_utf8_lossy(export.name());
            let name = name.trim_start_matches('_');
            name.starts_with("napi_") || name.starts_with("node_api_")
        })
        .map(|export| String::from_utf8_lossy(export.name()).to_string())
        .collect::<Vec<_>>();
    assert!(
        control_node_exports.is_empty(),
        "no-addon executable leaked Node-API exports: {control_node_exports:?}"
    );
    let control_sidecar = control_executable.with_file_name(format!(
        "{}.perry-native",
        control_executable.file_name().unwrap().to_string_lossy()
    ));
    assert!(
        !control_sidecar.exists(),
        "no-addon graph must not produce a Node-API sidecar"
    );
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"perry-node-api-e2e","private":true}"#,
    )
    .expect("remove unused native-addon policy for zero-byte control");
    let unconfigured_control = root.join(if cfg!(windows) {
        "control-unconfigured.exe"
    } else {
        "control-unconfigured"
    });
    let unconfigured_compile = compile_app(root, &control_entry, &unconfigured_control);
    assert!(
        unconfigured_compile.status.success(),
        "unconfigured no-addon control compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&unconfigured_compile.stdout),
        String::from_utf8_lossy(&unconfigured_compile.stderr)
    );
    assert_eq!(
        control_bytes.len(),
        std::fs::metadata(&unconfigured_control)
            .expect("stat unconfigured no-addon control")
            .len() as usize,
        "an unused perry.nativeAddons policy must have a zero-byte executable delta"
    );
    let host_delta = host_bytes.len().saturating_sub(control_bytes.len());
    assert!(
        host_delta <= 600 * 1024,
        "Node-API host executable delta is {host_delta} bytes; budget is 614400 bytes"
    );

    let sidecar = executable.with_file_name(format!(
        "{}.perry-native",
        executable.file_name().unwrap().to_string_lossy()
    ));
    let manifest_bytes =
        std::fs::read(sidecar.join("manifest.json")).expect("read Node-API sidecar manifest");
    let manifest: serde_json::Value =
        serde_json::from_slice(&manifest_bytes).expect("parse sidecar manifest");
    assert_eq!(manifest["napi_version"], 8);
    assert_eq!(
        manifest["addons"][0]["logical_id"],
        "fixture-addon/addon.node"
    );
    let relative_entry = manifest["addons"][0]["entry"]
        .as_str()
        .expect("manifest addon entry");
    let staged_addon = relative_entry
        .split('/')
        .fold(sidecar.clone(), |path, part| path.join(part));
    let staged_bytes = std::fs::read(&staged_addon).expect("read staged addon");
    let staged_hash = hex::encode(Sha256::digest(&staged_bytes));
    let file_record = manifest["addons"][0]["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["path"] == relative_entry)
        .expect("entry hash record");
    assert_eq!(file_record["sha256"], staged_hash);
    assert_eq!(file_record["size"], staged_bytes.len() as u64);

    let addon = object::File::parse(&*staged_bytes).expect("parse staged addon");
    let imports = addon.imports().expect("read staged addon imports");
    let imported_names = imports
        .iter()
        .map(|import| String::from_utf8_lossy(import.name()).to_string())
        .collect::<BTreeSet<_>>();
    for required in [
        "napi_create_int32",
        "napi_create_function",
        "napi_get_cb_info",
        "napi_get_value_int32",
        "napi_set_named_property",
    ] {
        assert!(
            imported_names.contains(required),
            "fixture must import `{required}` so the gate proves it resolved"
        );
        assert!(exported.contains(required), "host must export `{required}`");
    }
    #[cfg(windows)]
    for import in &imports {
        if String::from_utf8_lossy(import.name()).starts_with("napi_") {
            assert_eq!(
                String::from_utf8_lossy(import.library()).to_ascii_lowercase(),
                "app.exe",
                "Node-API import must resolve from the Perry host executable"
            );
        }
    }

    let run_output = run(Command::new(&executable), "compiled Node-API host");
    let stdout = String::from_utf8_lossy(&run_output.stdout);
    assert!(stdout.contains("node-api-answer 8523"), "stdout: {stdout}");
    assert!(stdout.contains("node-api-add 42"), "stdout: {stdout}");
    assert!(stdout.contains("node-api-cache true"), "stdout: {stdout}");

    let mut tampered = staged_bytes;
    tampered.push(0xA5);
    std::fs::write(&staged_addon, tampered).expect("tamper staged addon");
    let tampered_run = Command::new(&executable)
        .output()
        .expect("run tampered Node-API host");
    assert!(
        !tampered_run.status.success(),
        "tampered addon unexpectedly ran"
    );
    let diagnostic = format!(
        "{}{}",
        String::from_utf8_lossy(&tampered_run.stdout),
        String::from_utf8_lossy(&tampered_run.stderr)
    );
    assert!(diagnostic.contains("ERR_DLOPEN_FAILED"), "{diagnostic}");
    assert!(
        diagnostic.contains("expected") || diagnostic.contains("SHA-256"),
        "tamper diagnostic did not identify payload integrity: {diagnostic}"
    );

    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "perry-node-api-e2e",
  "private": true,
  "perry": {
    "compilePackages": ["fixture-addon"],
    "allow": { "compilePackages": ["fixture-addon"] }
  }
}"#,
    )
    .expect("remove addon authorization");
    let denied = compile_app(
        root,
        &entry,
        &root.join(if cfg!(windows) {
            "denied.exe"
        } else {
            "denied"
        }),
    );
    assert!(
        !denied.status.success(),
        "unlisted addon compile unexpectedly passed"
    );
    let denied_diagnostic = format!(
        "{}{}",
        String::from_utf8_lossy(&denied.stdout),
        String::from_utf8_lossy(&denied.stderr)
    );
    assert!(
        denied_diagnostic.contains("perry.nativeAddons"),
        "denied compile omitted policy guidance: {denied_diagnostic}"
    );
}

#[test]
fn published_napi_rs_addon_runs_sync_and_async_work() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"perry-napi-rs-e2e","private":true}"#,
    )
    .expect("write napi-rs project manifest");
    if !install_pinned_package(root, "@napi-rs/snappy@1.0.2", "published napi-rs") {
        return;
    }

    let modules = root.join("node_modules");
    let wrapper = modules.join("@napi-rs/snappy");
    let binding = find_node_file(&modules.join("@napi-rs"))
        .expect("npm installed a platform-specific napi-rs .node payload");
    assert!(
        !binding.starts_with(&wrapper),
        "fixture must exercise wrapper-to-platform-package authorization"
    );
    let from_scope = binding
        .strip_prefix(wrapper.parent().unwrap())
        .expect("binding under @napi-rs scope");
    let specifier = from_scope
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    std::fs::write(
        wrapper.join("index.js"),
        format!("module.exports = require('../{specifier}')\n"),
    )
    .expect("write static napi-rs platform wrapper");
    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "perry-napi-rs-e2e",
  "private": true,
  "perry": {
    "compilePackages": ["@napi-rs/snappy"],
    "allow": { "compilePackages": ["@napi-rs/snappy"] },
    "nativeAddons": ["@napi-rs/snappy"]
  }
}"#,
    )
    .expect("write napi-rs Perry policy");
    let entry = root.join("main.js");
    std::fs::write(
        &entry,
        r#"const snappy = require("@napi-rs/snappy")
const input = Buffer.from("napi-rs works")
const compressed = snappy.compressSync(input)
console.log("napi-rs-sync", snappy.uncompressSync(compressed).toString())
snappy.compress(input).then((asyncCompressed) => {
  console.log("napi-rs-async", snappy.uncompressSync(asyncCompressed).toString())
})
"#,
    )
    .expect("write napi-rs entry");

    let executable = root.join(if cfg!(windows) {
        "napi_rs.exe"
    } else {
        "napi_rs"
    });
    let compile = compile_app(root, &entry, &executable);
    assert!(
        compile.status.success(),
        "published napi-rs compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    let perry = run(
        Command::new(&executable),
        "published napi-rs Perry executable",
    );
    let perry_stdout = String::from_utf8_lossy(&perry.stdout);
    assert!(
        perry_stdout.contains("napi-rs-sync napi-rs works"),
        "stdout: {perry_stdout}"
    );
    assert!(
        perry_stdout.contains("napi-rs-async napi-rs works"),
        "stdout: {perry_stdout}"
    );

    if command_available("node") {
        let node = run(
            {
                let mut command = Command::new("node");
                command.current_dir(root).arg(&entry);
                command
            },
            "published napi-rs Node differential",
        );
        assert_eq!(
            perry.stdout, node.stdout,
            "Perry and Node must observe identical sync/async napi-rs results"
        );
    }
}

#[test]
fn parcel_watcher_facade_matches_real_watcher_snapshot_stream() {
    if !command_available("node") {
        if npm_e2e_required() {
            panic!("Node is required for the @parcel/watcher differential gate");
        }
        eprintln!(
            "SKIP: Node is unavailable; set PERRY_REQUIRE_NPM_E2E=1 to require @parcel/watcher"
        );
        return;
    }

    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"perry-parcel-watcher-e2e","private":true}"#,
    )
    .expect("write watcher project manifest");
    if !install_pinned_package(
        root,
        "@parcel/watcher@2.5.1",
        "@parcel/watcher differential",
    ) {
        return;
    }
    let watcher_node = find_node_file(&root.join("node_modules/@parcel"))
        .expect("pinned @parcel/watcher must install its real watcher.node payload");
    assert_eq!(
        watcher_node.file_name().and_then(|name| name.to_str()),
        Some("watcher.node")
    );
    let watcher_package_root = watcher_node
        .parent()
        .expect("watcher platform package root");
    let watcher_manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(watcher_package_root.join("package.json"))
            .expect("read watcher platform package manifest"),
    )
    .expect("parse watcher platform package manifest");
    let watcher_package = watcher_manifest["name"]
        .as_str()
        .expect("watcher platform package name");
    assert!(
        watcher_package.starts_with("@parcel/watcher-"),
        "unexpected watcher platform package `{watcher_package}`"
    );
    std::fs::write(
        root.join("package.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "name": "perry-parcel-watcher-e2e",
            "private": true,
            "perry": {
                "compilePackages": [watcher_package],
                "allow": { "compilePackages": [watcher_package] },
                "nativeAddons": [watcher_package]
            }
        }))
        .unwrap(),
    )
    .expect("write exact watcher platform policy");

    let script = |tree: &Path, snapshot: &Path| {
        let tree = serde_json::to_string(tree.to_string_lossy().as_ref()).unwrap();
        let snapshot = serde_json::to_string(snapshot.to_string_lossy().as_ref()).unwrap();
        let watcher_package = serde_json::to_string(watcher_package).unwrap();
        format!(
            r#"const fs = require("fs")
const path = require("path")
const watcher = require({watcher_package})
const root = {tree}
const snapshot = {snapshot}
fs.mkdirSync(root, {{ recursive: true }})
fs.writeFileSync(path.join(root, "existing.txt"), "one")
fs.writeFileSync(path.join(root, "renamed-before.txt"), "rename")
fs.writeFileSync(path.join(root, "deleted.txt"), "delete")
watcher.writeSnapshot(root, snapshot, {{}}).then(() => {{
  fs.writeFileSync(path.join(root, "existing.txt"), "a longer updated value")
  fs.writeFileSync(path.join(root, "created.txt"), "created")
  fs.renameSync(path.join(root, "renamed-before.txt"), path.join(root, "renamed-after.txt"))
  fs.unlinkSync(path.join(root, "deleted.txt"))
  return watcher.getEventsSince(root, snapshot, {{}})
}}).then((events) => {{
  const normalized = events.map((event) => ({{
    type: event.type,
    path: path.relative(root, event.path).replace(/\\/g, "/")
  }})).sort((left, right) => left.path.localeCompare(right.path) || left.type.localeCompare(right.type))
  console.log(JSON.stringify(normalized))
}})
"#
        )
    };

    let node_entry = root.join("watcher-node.js");
    std::fs::write(
        &node_entry,
        script(&root.join("tree-node"), &root.join("node.snapshot")),
    )
    .expect("write Node watcher differential entry");
    let node = run(
        {
            let mut command = Command::new("node");
            command.current_dir(root).arg(&node_entry);
            command
        },
        "real @parcel/watcher snapshot stream under Node",
    );

    let perry_entry = root.join("watcher-perry.js");
    std::fs::write(
        &perry_entry,
        script(&root.join("tree-perry"), &root.join("perry.snapshot")),
    )
    .expect("write Perry watcher differential entry");
    let executable = root.join(if cfg!(windows) {
        "watcher-perry.exe"
    } else {
        "watcher-perry"
    });
    let compile = compile_app(root, &perry_entry, &executable);
    assert!(
        compile.status.success(),
        "@parcel/watcher facade compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    let executable_bytes = std::fs::read(&executable).expect("read watcher facade executable");
    let object = object::File::parse(&*executable_bytes).expect("parse watcher facade executable");
    let leaked_node_api = object
        .exports()
        .expect("read watcher facade exports")
        .into_iter()
        .map(|export| String::from_utf8_lossy(export.name()).to_string())
        .filter(|name| {
            let name = name.trim_start_matches('_');
            name.starts_with("napi_") || name.starts_with("node_api_")
        })
        .collect::<Vec<_>>();
    assert!(
        leaked_node_api.is_empty(),
        "the @parcel/watcher facade must take precedence over Node-API hosting: {leaked_node_api:?}"
    );
    let sidecar = executable.with_file_name(format!(
        "{}.perry-native",
        executable.file_name().unwrap().to_string_lossy()
    ));
    assert!(
        !sidecar.exists(),
        "the @parcel/watcher facade must not ship watcher.node"
    );

    let perry = run(
        {
            let mut command = Command::new(&executable);
            command.current_dir(root);
            command
        },
        "Perry @parcel/watcher facade snapshot stream",
    );
    assert_eq!(
        perry.stdout, node.stdout,
        "Perry facade and real watcher.node must produce the same coalesced snapshot stream\ncompile:\n{}",
        String::from_utf8_lossy(&compile.stdout)
    );
}
