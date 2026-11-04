use std::path::{Path, PathBuf};
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn write_minicord_fixture(root: &Path) {
    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "require-package-reducer",
  "type": "module",
  "perry": {
    "compilePackages": ["minicord"],
    "allow": { "compilePackages": ["minicord"] }
  }
}"#,
    )
    .expect("write consumer package.json");

    let pkg = root.join("node_modules").join("minicord");
    std::fs::create_dir_all(&pkg).expect("mkdir minicord");
    std::fs::write(
        pkg.join("package.json"),
        r#"{ "name": "minicord", "version": "1.0.0", "main": "index.ts", "types": "index.ts" }"#,
    )
    .expect("write minicord package.json");
    std::fs::write(
        pkg.join("index.ts"),
        r#"
export class Client {
  tag: string;
  constructor(tag: string) {
    this.tag = tag;
  }
  login(): string {
    return "login:" + this.tag;
  }
}
export const version = "mini-1";
export function make(name: string): string {
  return "make:" + name;
}
"#,
    )
    .expect("write minicord index");

    std::fs::write(
        root.join("local.ts"),
        r#"
export const localValue = "local-ok";
export function localCall(value: string): string {
  return "local:" + value;
}
"#,
    )
    .expect("write local module");
}

fn compile_and_run(root: &Path, entry: &Path) -> String {
    let output = root.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
        .arg("compile")
        .arg(entry)
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
    String::from_utf8(run.stdout).expect("stdout utf8")
}

#[test]
fn create_require_literal_package_and_file_resolve_to_compiled_modules() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write_minicord_fixture(root);

    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        r#"
import { createRequire } from "node:module";

const req = createRequire(import.meta.url);
console.log("builtin:", typeof req("node:path").join);

const require = createRequire(import.meta.url);
const Mini = require("minicord");
const Local = require("./local");

const client = new Mini.Client("A");
console.log("package:", Mini.version, Mini.make("B"), client.login());
console.log("file:", Local.localValue, Local.localCall("C"));
"#,
    )
    .expect("write entry");

    assert_eq!(
        compile_and_run(root, &entry),
        "builtin: function\npackage: mini-1 make:B login:A\nfile: local-ok local:C\n"
    );
}

#[test]
fn direct_literal_require_package_and_file_resolve_to_compiled_modules() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write_minicord_fixture(root);

    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        r#"
const Mini = require("minicord");
const { localValue, localCall } = require("./local");

const client = new Mini.Client("A");
console.log("package:", Mini.version, Mini.make("B"), client.login());
console.log("file:", localValue, localCall("C"));
"#,
    )
    .expect("write entry");

    assert_eq!(
        compile_and_run(root, &entry),
        "package: mini-1 make:B login:A\nfile: local-ok local:C\n"
    );
}

/// Tier 1 of #5389 (fixes #5373): a bare/computed `require(expr)` inside a
/// compiled `compilePackages` module must bind to a createRequire-backed closure
/// instead of throwing `ReferenceError: require is not defined`. Builtins resolve
/// by string, `typeof require` is "function", a non-builtin package specifier
/// throws Node-compatible MODULE_NOT_FOUND (not a ReferenceError), and a
/// shadowing local `require` still wins.
#[test]
fn ambient_require_in_compiled_package_resolves_builtins_without_reference_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "ambient-require-consumer",
  "type": "module",
  "perry": {
    "compilePackages": ["amblib"],
    "allow": { "compilePackages": ["amblib"] }
  }
}"#,
    )
    .expect("write consumer package.json");

    let pkg = root.join("node_modules").join("amblib");
    std::fs::create_dir_all(&pkg).expect("mkdir amblib");
    std::fs::write(
        pkg.join("package.json"),
        r#"{ "name": "amblib", "version": "1.0.0", "main": "index.ts", "types": "index.ts" }"#,
    )
    .expect("write amblib package.json");
    // The computed specifiers are built at runtime so the literal-require
    // rewrites can't fire — they exercise the ambient closure directly.
    std::fs::write(
        pkg.join("index.ts"),
        r#"
export function probe(): string {
  const builtin = ["n", "o", "d", "e", ":", "o", "s"].join("");
  let viaRequire: string;
  try {
    require(builtin);
    viaRequire = "builtin-ok";
  } catch (e) {
    viaRequire = "builtin-threw-" + (e as Error).name;
  }

  const pkgSpec = ["l", "o", "d", "a", "s", "h"].join("");
  let viaPackage: string;
  try {
    require(pkgSpec);
    viaPackage = "pkg-ok";
  } catch (e) {
    viaPackage = (e as Error).name + ":" + ((e as any).code ?? "(none)");
  }

  return `typeof=${typeof require} | ${viaRequire} | ${viaPackage}`;
}

export function shadowed(): string {
  const require = (id: string) => "shadow:" + id;
  return require("zzz");
}
"#,
    )
    .expect("write amblib index");

    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        r#"
import { probe, shadowed } from "amblib";
console.log(probe());
console.log(shadowed());
"#,
    )
    .expect("write entry");
    let output = root.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
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
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert_eq!(
        stdout,
        "typeof=function | builtin-ok | Error:MODULE_NOT_FOUND\nshadow:zzz\n"
    );
}

/// Tier 2 of #5389: a **computed** `require(expr)` whose specifier const-folds
/// (ternary of literals, module-const, or a directory-anchored template glob) to
/// a finite set of compiled-package modules resolves **synchronously** to the
/// target namespace — reusing the dynamic-`import()` resolver but returning the
/// value directly (no Promise). A specifier that does not const-fold falls back
/// to the Tier-1 ambient require (builtins resolve by string; unknown packages
/// throw Node-compatible `MODULE_NOT_FOUND`).
#[test]
fn computed_require_const_folds_to_compiled_package_modules() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "computed-require-consumer",
  "type": "module",
  "perry": {
    "compilePackages": ["dynlib"],
    "allow": { "compilePackages": ["dynlib"] }
  }
}"#,
    )
    .expect("write consumer package.json");

    let pkg = root.join("node_modules").join("dynlib");
    let plugins = pkg.join("plugins");
    std::fs::create_dir_all(&plugins).expect("mkdir dynlib/plugins");
    std::fs::write(
        pkg.join("package.json"),
        r#"{ "name": "dynlib", "version": "1.0.0", "main": "index.ts", "types": "index.ts" }"#,
    )
    .expect("write dynlib package.json");
    std::fs::write(pkg.join("alpha.ts"), "export const tag = \"ALPHA\";\n").expect("write alpha");
    std::fs::write(pkg.join("beta.ts"), "export const tag = \"BETA\";\n").expect("write beta");
    std::fs::write(
        plugins.join("foo.ts"),
        "export const id = \"plugin-foo\";\n",
    )
    .expect("write plugin foo");
    std::fs::write(
        pkg.join("index.ts"),
        r#"
// ternary of relative literals -> const-folds to a 2-element set.
export function pick(flag: boolean): string {
  const m = require(flag ? "./alpha" : "./beta");
  return m.tag;
}
// module-const literal -> const-folds to a single target.
export function viaConst(): string {
  const SPEC = "./alpha";
  return "const:" + require(SPEC).tag;
}
// directory-anchored template -> globs the plugins dir (runtime string must
// equal the file specifier, including extension — same contract as import()).
export function plugin(name: string): string {
  return require(`./plugins/${name}`).id;
}
// not const-foldable -> ambient fallback (builtin resolves by string).
export function builtinComputed(): string {
  const spec = ["n", "o", "d", "e", ":", "o", "s"].join("");
  return "os:" + typeof require(spec).platform;
}
"#,
    )
    .expect("write dynlib index");

    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        r#"
import { pick, viaConst, plugin, builtinComputed } from "dynlib";
console.log(pick(true), pick(false));
console.log(viaConst());
console.log(plugin("foo.ts"));
console.log(builtinComputed());
"#,
    )
    .expect("write entry");

    let output = root.join("main_bin");
    let compile = Command::new(perry_bin())
        .current_dir(root)
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
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert_eq!(stdout, "ALPHA BETA\nconst:ALPHA\nplugin-foo\nos:function\n");
}

#[test]
fn deep_commonjs_require_preserves_export_values_identity_and_timing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let package = root.join("node_modules/deep-cjs");
    std::fs::create_dir_all(package.join("lib/plain")).unwrap();
    std::fs::write(
        root.join("package.json"),
        r#"{
        "type":"module", "perry": {
            "compilePackages":["deep-cjs"],
            "allow":{"compilePackages":["deep-cjs"]}
        }
    }"#,
    )
    .unwrap();
    for (file, source) in [
        (
            "package.json",
            r#"{"name":"deep-cjs","version":"1.0.0","main":"index.js"}"#,
        ),
        ("index.js", "module.exports = require('./lib/plain');"),
        (
            "lib/plain/index.js",
            r#"
console.log('class loaded');
class Plain { constructor(value) { this.value = value; } get() { return this.value; } }
module.exports = Plain;
"#,
        ),
        (
            "lib/factory.js",
            "module.exports = function(value) { return 'factory:' + value; };",
        ),
        (
            "lib/object.js",
            "module.exports = { default: 'inner', named: 42 };",
        ),
        ("lib/number.js", "module.exports = 17;"),
        (
            "lib/unused.js",
            "throw new Error('unused module ran'); module.exports = {};",
        ),
        (
            "lib/esm.mjs",
            "export default 'esm-default'; export const named = 'esm-named';",
        ),
    ] {
        std::fs::write(package.join(file), source).unwrap();
    }
    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        r#"
import { createRequire as makeRequire } from 'node:module';
const req = makeRequire(import.meta.url);
console.log('entry');
if (false) req('deep-cjs/lib/unused');
const Plain: any = req('deep-cjs/lib/plain');
console.log(typeof Plain, new Plain('deep').get());
console.log('identity', Plain === req('deep-cjs/lib/plain'), Plain === req('deep-cjs'));
console.log(req('deep-cjs/lib/factory')('ok'));
const object = req('deep-cjs/lib/object');
console.log('object', object.default, object.named);
console.log('number', req('deep-cjs/lib/number'));
const esm = req('deep-cjs/lib/esm.mjs');
console.log('esm', esm.default, esm.named);
"#,
    )
    .unwrap();
    assert_eq!(compile_and_run(root, &entry),
        "entry\nclass loaded\nfunction deep\nidentity true true\nfactory:ok\nobject inner 42\nnumber 17\nesm esm-default esm-named\n");
}

#[test]
fn create_require_before_the_first_commonjs_load_holds_the_program_cache() {
    // The entry is ESM: the CommonJS support module would come in with the
    // first deferred require, after `createRequire` ran. `require.cache` must
    // already be the program's cache, the object the module's record is in.
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(
        root.join("mod.cjs"),
        "globalThis.__loads = (globalThis.__loads || 0) + 1;\nmodule.exports = { self: module, loads: globalThis.__loads };\n",
    )
    .unwrap();
    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        r#"
import { createRequire } from "node:module";
const require = createRequire(import.meta.url);
const path = require.resolve("./mod.cjs");
const m = require("./mod.cjs");
console.log("one cache", require.cache === createRequire(import.meta.url).cache);
const rec = require.cache[path];
console.log("record", rec === m.self, rec !== undefined && rec.exports === m, m.loads);
delete require.cache[path];
const m2 = require("./mod.cjs");
console.log("reload", m2 !== m, m2.loads, require.cache[path].exports === m2);
"#,
    )
    .unwrap();
    assert_eq!(
        compile_and_run(root, &entry),
        "one cache true\nrecord true true 1\nreload true 2 true\n"
    );
}

/// A `createRequire` call of a package Perry implements natively (`node-pty`)
/// gets the native module, the one a static import gets, even with the npm
/// package installed in `node_modules`. The stub package's `spawn` throws, so
/// reaching it fails the test.
#[cfg(unix)]
#[test]
fn create_require_of_native_package_is_the_native_module() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "native-package-create-require", "type": "module" }"#,
    )
    .expect("write consumer package.json");
    let package = root.join("node_modules").join("node-pty");
    std::fs::create_dir_all(&package).expect("mkdir node-pty stub");
    std::fs::write(
        package.join("package.json"),
        r#"{ "name": "node-pty", "version": "1.0.0", "main": "index.js" }"#,
    )
    .expect("write stub package.json");
    std::fs::write(
        package.join("index.js"),
        "exports.spawn = function () { throw new Error(\"npm stub reached\"); };\n",
    )
    .expect("write stub index");

    let entry = root.join("main.ts");
    std::fs::write(
        &entry,
        r#"
import * as NodeModule from "node:module";

const requirePty = NodeModule.createRequire(import.meta.url);
const pty: any = requirePty("node-pty");
const proc = pty.spawn("/bin/sh", ["-c", "echo one-$((6*7))"], {
  name: "xterm",
  cols: 80,
  rows: 24,
  cwd: "/",
  env: { PATH: "/usr/bin:/bin" },
});
let out = "";
proc.onData((data: string) => {
  out += data;
});
proc.onExit(({ exitCode }: { exitCode: number }) => {
  console.log(`saw: ${out.includes("one-42")}`);
  console.log(`exit ${exitCode}`);
});
"#,
    )
    .expect("write entry");

    assert_eq!(compile_and_run(root, &entry), "saw: true\nexit 0\n");
}

/// One specifier, one resolution: however a module obtains `createRequire`
/// from `"module"` (default import, CommonJS destructuring or binding, inline
/// `require("module").createRequire`) and however it names itself as the base
/// (`import.meta.url`, `new URL(".", import.meta.url)`,
/// `fileURLToPath(import.meta.url)`, `__filename`), a literal require of an
/// installed CommonJS package outside `compilePackages` loads it, as Node does.
#[test]
fn every_create_require_form_loads_an_installed_commonjs_package() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "create-require-forms", "private": true, "type": "module" }"#,
    )
    .expect("write consumer package.json");
    let package = root.join("node_modules/plain-cjs");
    std::fs::create_dir_all(package.join("lib")).expect("mkdir plain-cjs");
    std::fs::write(
        package.join("package.json"),
        r#"{ "name": "plain-cjs", "version": "1.0.0", "main": "lib/index.js" }"#,
    )
    .expect("write plain-cjs package.json");
    std::fs::write(
        package.join("lib/index.js"),
        "exports.hello = function () { return \"hi\"; };\n",
    )
    .expect("write plain-cjs index");

    let esm = root.join("main.ts");
    std::fs::write(
        &esm,
        r#"import Module from "node:module";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
const byDefault = Module.createRequire(import.meta.url);
const byDirectoryUrl = createRequire(new URL(".", import.meta.url));
const byPath = createRequire(fileURLToPath(import.meta.url));
console.log("default", byDefault("plain-cjs").hello());
console.log("directory-url", byDirectoryUrl("plain-cjs").hello());
console.log("path", byPath("plain-cjs").hello());
"#,
    )
    .expect("write ESM entry");
    let expected_esm = "default hi\ndirectory-url hi\npath hi\n";
    assert_eq!(compile_and_run(root, &esm), expected_esm);

    let commonjs = root.join("main.cts");
    std::fs::write(
        &commonjs,
        r#"const { createRequire } = require("module");
const NodeModule = require("node:module");
const byDestructuring = createRequire(__filename);
const byBinding = NodeModule.createRequire(__filename);
const inline = require("node:module").createRequire(__filename);
console.log("destructuring", byDestructuring("plain-cjs").hello());
console.log("binding", byBinding("plain-cjs").hello());
console.log("inline", inline("plain-cjs").hello());
"#,
    )
    .expect("write CommonJS entry");
    let expected_commonjs = "destructuring hi\nbinding hi\ninline hi\n";
    assert_eq!(compile_and_run(root, &commonjs), expected_commonjs);

    if Command::new("node").arg("--version").output().is_ok() {
        for (entry, expected) in [(&esm, expected_esm), (&commonjs, expected_commonjs)] {
            let node = Command::new("node")
                .current_dir(root)
                .arg(entry)
                .output()
                .expect("run node");
            assert_eq!(
                String::from_utf8_lossy(&node.stdout),
                expected,
                "the expected transcript must be Node's\nstderr:\n{}",
                String::from_utf8_lossy(&node.stderr)
            );
        }
    }
}
