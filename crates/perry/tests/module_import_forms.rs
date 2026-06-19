use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Once;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize workspace root")
}

fn target_debug_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("target"))
        .join("debug")
}

fn ensure_runtime_archive() {
    static BUILD_RUNTIME: Once = Once::new();
    BUILD_RUNTIME.call_once(|| {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let build = Command::new(cargo)
            .current_dir(workspace_root())
            .arg("build")
            .arg("-p")
            .arg("perry-runtime")
            .output()
            .expect("run cargo build -p perry-runtime");
        assert!(
            build.status.success(),
            "cargo build -p perry-runtime failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&build.stdout),
            String::from_utf8_lossy(&build.stderr)
        );
    });
}

fn runtime_dir() -> PathBuf {
    ensure_runtime_archive();
    target_debug_dir()
}

fn write_mini_package(root: &Path) {
    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "module-import-forms",
  "type": "module",
  "perry": {
    "compilePackages": ["mini-discord-like"],
    "allow": { "compilePackages": ["mini-discord-like"] }
  }
}"#,
    )
    .expect("write consumer package.json");

    let pkg = root.join("node_modules").join("mini-discord-like");
    std::fs::create_dir_all(&pkg).expect("mkdir package");
    std::fs::write(
        pkg.join("package.json"),
        r#"{
  "name": "mini-discord-like",
  "version": "1.0.0",
  "type": "module",
  "exports": "./index.js"
}"#,
    )
    .expect("write package.json");
    std::fs::write(
        pkg.join("index.js"),
        r#"
export const version = "mini-1";
export class Client {
  constructor(label = "client") {
    this.label = label;
  }
  login() {
    return `login:${this.label}`;
  }
}
export const Routes = {
  channel(id) {
    return `/channels/${id}`;
  },
};
"#,
    )
    .expect("write index.js");
}

fn write_mini_cjs_package(root: &Path) {
    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "module-import-forms",
  "type": "module",
  "perry": {
    "compilePackages": ["mini-discord-cjs"],
    "allow": { "compilePackages": ["mini-discord-cjs"] }
  }
}"#,
    )
    .expect("write consumer package.json");

    let pkg = root.join("node_modules").join("mini-discord-cjs");
    std::fs::create_dir_all(&pkg).expect("mkdir package");
    std::fs::write(
        pkg.join("package.json"),
        r#"{ "name": "mini-discord-cjs", "version": "1.0.0", "main": "index.js" }"#,
    )
    .expect("write package.json");
    std::fs::write(
        pkg.join("index.js"),
        r#"
exports.version = "mini-cjs-1";
class Client {
  constructor(label) {
    this.label = label || "client";
  }
  login() {
    return "login:" + this.label;
  }
}
exports.Client = Client;
exports.Routes = {
  channel(id) {
    return "/channels/" + id;
  },
};
"#,
    )
    .expect("write index.js");
}

fn write_mini_require_target(root: &Path) {
    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "module-import-forms",
  "type": "module",
  "perry": {
    "compilePackages": ["mini-require-target"],
    "allow": { "compilePackages": ["mini-require-target"] }
  }
}"#,
    )
    .expect("write consumer package.json");

    let pkg = root.join("node_modules").join("mini-require-target");
    std::fs::create_dir_all(&pkg).expect("mkdir package");
    std::fs::write(
        pkg.join("package.json"),
        r#"{ "name": "mini-require-target", "version": "1.0.0", "main": "index.js" }"#,
    )
    .expect("write package.json");
    std::fs::write(
        pkg.join("index.js"),
        r#"exports.version = "require-target-1";"#,
    )
    .expect("write index.js");
}

fn write_mini_star_barrel_packages(root: &Path) {
    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "module-import-forms",
  "type": "module",
  "perry": {
    "compilePackages": ["mini-root-barrel"],
    "allow": { "compilePackages": ["mini-root-barrel"] }
  }
}"#,
    )
    .expect("write consumer package.json");

    let root_pkg = root.join("node_modules").join("mini-root-barrel");
    std::fs::create_dir_all(&root_pkg).expect("mkdir root package");
    std::fs::write(
        root_pkg.join("package.json"),
        r#"{
  "name": "mini-root-barrel",
  "version": "1.0.0",
  "type": "module",
  "exports": "./index.ts",
  "dependencies": { "mini-core-barrel": "1.0.0" }
}"#,
    )
    .expect("write root package.json");
    std::fs::write(
        root_pkg.join("index.ts"),
        r#"export * from "mini-core-barrel";"#,
    )
    .expect("write root index.ts");

    let core_pkg = root.join("node_modules").join("mini-core-barrel");
    std::fs::create_dir_all(&core_pkg).expect("mkdir core package");
    std::fs::write(
        core_pkg.join("package.json"),
        r#"{
  "name": "mini-core-barrel",
  "version": "1.0.0",
  "type": "module",
  "exports": "./index.ts"
}"#,
    )
    .expect("write core package.json");
    std::fs::write(
        core_pkg.join("index.ts"),
        r#"
export class Client {
  constructor(label = "client") {
    this.label = label;
  }
  login() {
    return `login:${this.label}`;
  }
}

export function needsArgument(value) {
  return value.length;
}

export const version = "star-1";
"#,
    )
    .expect("write core index.ts");
}

#[test]
fn dynamic_package_import_returns_the_same_namespace_as_static_imports() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write_mini_package(root);

    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        r#"
import { Client, Routes, version } from "mini-discord-like";
import * as StaticNS from "mini-discord-like";

const DynamicNS = await import("mini-discord-like");

console.log("static-named", version, new Client("named").login(), Routes.channel("42"));
console.log("static-ns", StaticNS.version, new StaticNS.Client("ns").login(), StaticNS.Routes.channel("43"));
console.log("dynamic-ns", DynamicNS.version, new DynamicNS.Client("dyn").login(), DynamicNS.Routes.channel("44"));
"#,
    )
    .expect("write entry");

    let output = root.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "static-named mini-1 login:named /channels/42\n\
         static-ns mini-1 login:ns /channels/43\n\
         dynamic-ns mini-1 login:dyn /channels/44\n"
    );
}

#[test]
fn dynamic_only_package_import_materializes_package_namespace() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write_mini_package(root);

    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        r#"
const DynamicNS = await import("mini-discord-like");
console.log("dynamic-only", DynamicNS.version, new DynamicNS.Client("dyn").login(), DynamicNS.Routes.channel("44"));
"#,
    )
    .expect("write entry");

    let output = root.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "dynamic-only mini-1 login:dyn /channels/44\n"
    );
}

#[test]
fn dynamic_package_import_of_cjs_package_matches_static_imports() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write_mini_cjs_package(root);

    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        r#"
import { Client, Routes, version } from "mini-discord-cjs";
import * as StaticNS from "mini-discord-cjs";

const DynamicNS = await import("mini-discord-cjs");

console.log("static-named", version, new Client("named").login(), Routes.channel("42"));
console.log("static-ns", StaticNS.version, new StaticNS.Client("ns").login(), StaticNS.Routes.channel("43"));
console.log("dynamic-ns", DynamicNS.version, new DynamicNS.Client("dyn").login(), DynamicNS.Routes.channel("44"));
"#,
    )
    .expect("write entry");

    let output = root.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "static-named mini-cjs-1 login:named /channels/42\n\
         static-ns mini-cjs-1 login:ns /channels/43\n\
         dynamic-ns mini-cjs-1 login:dyn /channels/44\n"
    );
}

#[test]
fn dynamic_import_of_star_barrel_does_not_invoke_reexported_functions() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write_mini_star_barrel_packages(root);

    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        r#"
import { Client, needsArgument, version } from "mini-root-barrel";

const DynamicNS = await import("mini-root-barrel");

if (DynamicNS.Client !== Client) {
  throw new Error("dynamic star barrel should expose the same Client binding");
}
if (DynamicNS.needsArgument !== needsArgument) {
  throw new Error("dynamic star barrel should expose the same function binding");
}
if (Object.prototype.hasOwnProperty.call(DynamicNS, "default")) {
  throw new Error("dynamic star barrel should not expose a default export");
}

console.log("star-barrel", version, new DynamicNS.Client("dyn").login(), DynamicNS.needsArgument("abcd"));
"#,
    )
    .expect("write entry");

    let output = root.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "star-barrel star-1 login:dyn 4\n"
    );
}

/// #5216: a string-literal `require("<native>")` of a statically resolvable
/// Node builtin lowers to the same module-namespace value `import * as ns from
/// "<native>"` binds — so the namespace, destructured-member, and inline-member
/// shapes all compile and dispatch through the existing native-module
/// machinery, identically to the equivalent namespace import.
#[test]
fn require_native_builtin_lowers_like_namespace_import() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "require-builtins", "type": "module" }"#,
    )
    .expect("write package.json");

    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        r#"
import * as rlImport from "readline";

const readline = require("readline");
const { createInterface } = require("readline");

console.log("ns", typeof readline.createInterface);
console.log("destructured", typeof createInterface);
console.log("import-equiv", typeof rlImport.createInterface);
console.log("inline-eol", typeof require("node:os").EOL);
console.log("inline-platform", require("node:os").platform() === process.platform);
"#,
    )
    .expect("write entry");

    let output = root.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed (require of a resolvable builtin must NOT refuse)\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "ns function\n\
         destructured function\n\
         import-equiv function\n\
         inline-eol string\n\
         inline-platform true\n"
    );
}

#[test]
fn create_require_package_specifier_resolves_to_compiled_namespace() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write_mini_require_target(root);

    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        r#"
import { createRequire } from "node:module";
import { version } from "mini-require-target";

const require = createRequire(import.meta.url);
console.log("esm-version", version);
console.log("require-version", require("mini-require-target").version);
"#,
    )
    .expect("write entry");

    let output = root.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .env("PERRY_NO_AUTO_OPTIMIZE", "1")
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output).output().expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "esm-version require-target-1\nrequire-version require-target-1\n"
    );
}
