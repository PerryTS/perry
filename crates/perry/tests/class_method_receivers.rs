//! Strict class method receivers survive call, apply, bind and detached calls.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn successful(command: &mut Command, subject: &str) -> Output {
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("{subject}: {error}"));
    assert!(
        output.status.success(),
        "{subject} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

#[test]
fn class_method_receivers_match_node() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let entry = root.join("main.ts");
    std::fs::write(&entry, r#"
class P { get() { return typeof this; } identity() { return this; } }
const values: any[] = [undefined, null, true, false, 0, -0, 42, NaN, Number.MIN_VALUE, 'text', Symbol('x'), 9n, {}, [], new P(), function f() {}];
for (const value of values) {
  console.log(typeof value, P.prototype.get.call(value), Object.is(P.prototype.identity.call(value), value));
}
console.log('apply', P.prototype.get.apply(undefined), P.prototype.get.apply(null));
console.log('bind', P.prototype.get.bind(undefined)(), P.prototype.get.bind(null)());
const detached = P.prototype.get;
console.log('detached', detached());

class Rest {
  inspect(...items: any[]) { return [typeof this, items.length, items[0]].join(':'); }
}
console.log('rest', Rest.prototype.inspect.call(undefined, 5, 6));
console.log('rest-null', Rest.prototype.inspect.apply(null, [7]));
const proxied = new Proxy({}, {});
console.log('proxy', P.prototype.get.call(proxied), P.prototype.identity.call(proxied) === proxied);
"#).unwrap();
    let node = successful(
        Command::new("node").current_dir(&root).arg(&entry),
        "Node oracle",
    );
    let transcript = String::from_utf8_lossy(&node.stdout);
    assert!(transcript.contains("undefined undefined true\n"));
    assert!(transcript.contains("object object true\n"));
    assert!(
        !transcript.contains("false"),
        "all receiver identities must match: {transcript}"
    );
    let compiler = PathBuf::from(env!("CARGO_BIN_EXE_perry"));
    let runtime = std::env::var_os("PERRY_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| compiler.parent().unwrap().to_path_buf());
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let executable = root.join(if cfg!(windows) {
        "app.exe"
    } else {
        "app-native"
    });
    successful(
        Command::new(compiler)
            .current_dir(&root)
            .env("PERRY_RUNTIME_DIR", runtime)
            .env("PERRY_WORKSPACE_ROOT", workspace)
            .args(["compile", "--no-cache", "--no-auto-optimize"])
            .arg(&entry)
            .arg("-o")
            .arg(&executable),
        "Perry compile",
    );
    let native = successful(
        Command::new(executable).current_dir(&root),
        "Perry executable",
    );
    assert_eq!(
        native.stdout, node.stdout,
        "strict class methods must preserve Node's receiver types and identities"
    );
}
