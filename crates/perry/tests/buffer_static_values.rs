//! Buffer static methods remain callable when read as values (#11257).

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
fn buffer_static_values_match_node() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let entry = root.join("main.ts");
    std::fs::write(&entry, r#"const from = Buffer.from;
const alloc = Buffer.alloc;
const concat = Buffer.concat;
console.log('types', typeof from, typeof alloc, typeof concat, typeof Buffer.isBuffer);
console.log('identity', from === (Buffer as any)['from'], alloc === globalThis.Buffer.alloc, concat === globalThis.Buffer.concat);
if (typeof from === 'function' && typeof alloc === 'function' && typeof concat === 'function') {
  console.log('detached', from('ab').length, alloc(3).length, concat([from('a'), from('bc')]).length);
  console.log('metadata', from.name, alloc.name, concat.name);
}
const { from: destructured } = Buffer;
console.log('destructured', typeof destructured, destructured('abc').length);
console.log('direct', Buffer.from('ab').length, Buffer.alloc(2).length, Buffer.concat([Buffer.from('ab')]).length);
console.log('array', typeof Array.from, Array.from('ab').length);
{
 const Buffer = { from: () => 'local', alloc: () => 'local-alloc', concat: () => 'local-concat' };
 const local = Buffer.from;
 const localAlloc = Buffer.alloc;
 const localConcat = Buffer.concat;
 console.log('shadow', local(), localAlloc(), localConcat());
}
const real: any = Buffer;
real.extra = () => 'extra';
console.log('extra', typeof Buffer.extra, (Buffer as any)['extra']());
delete real.extra;
"#).unwrap();
    let node = successful(
        Command::new("node").current_dir(&root).arg(&entry),
        "Node oracle",
    );
    let expected = r#"types function function function function
identity true true true
detached 2 3 3
metadata from alloc concat
destructured function 3
direct 2 2 2
array function 2
shadow local local-alloc local-concat
extra function extra
"#;
    assert_eq!(
        node.stdout,
        expected.as_bytes(),
        "Node fixture must exercise detached calls and identities"
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
        "Buffer static value reads must match Node"
    );
}
