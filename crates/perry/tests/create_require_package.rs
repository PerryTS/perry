use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn create_require_literal_package_and_file_resolve_to_compiled_modules() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    std::fs::write(
        root.join("package.json"),
        r#"{
  "name": "create-require-package-reducer",
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
        "builtin: function\npackage: mini-1 make:B login:A\nfile: local-ok local:C\n"
    );
}

/// Tier 1 of #5389 (fixes #5373): a bare/computed `require(expr)` inside a
/// compiled `compilePackages` module must bind to a createRequire-backed closure
/// instead of throwing `ReferenceError: require is not defined`. Builtins resolve
/// by string, `typeof require` is "function", a non-builtin package specifier
/// throws the descriptive ERR_PERRY_UNSUPPORTED_CREATE_REQUIRE (not a
/// ReferenceError), and a shadowing local `require` still wins.
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
        "typeof=function | builtin-ok | Error:ERR_PERRY_UNSUPPORTED_CREATE_REQUIRE\nshadow:zzz\n"
    );
}
