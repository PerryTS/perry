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
