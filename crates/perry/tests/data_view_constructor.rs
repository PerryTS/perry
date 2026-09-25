//! DataView constructor lookup follows the actual prototype chain.

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
fn data_view_constructor_matches_node() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let entry = root.join("main.ts");
    std::fs::write(&entry, r#"
class SubDV extends DataView {}
class DeepDV extends SubDV {}
class SubU8 extends Uint8Array {}
function report(label: string, value: any, ctor: any) {
  console.log(label, value.constructor.name, value.constructor === ctor, Object.getPrototypeOf(value) === ctor.prototype);
}
report('plain', new DataView(new ArrayBuffer(4)), DataView);
report('sub', new SubDV(new ArrayBuffer(4)), SubDV);
report('deep', new DeepDV(new ArrayBuffer(4)), DeepDV);
report('control', new SubU8(4), SubU8);
function Replacement() {}
const own: any = new SubDV(new ArrayBuffer(4));
Object.defineProperty(own, 'constructor', { value: Replacement, configurable: true });
console.log('own', own.constructor === Replacement);
const inherited: any = new SubDV(new ArrayBuffer(4));
Object.defineProperty(SubDV.prototype, 'constructor', { value: Replacement, configurable: true, writable: true });
console.log('inherited', inherited.constructor === Replacement);
const noProto: any = new SubDV(new ArrayBuffer(4));
Object.setPrototypeOf(noProto, null);
console.log('null-proto', typeof noProto.constructor);
const missing: any = new SubDV(new ArrayBuffer(4));
Object.setPrototypeOf(missing, Object.create(null));
console.log('missing', typeof missing.constructor);
const accessor: any = new SubDV(new ArrayBuffer(4));
Object.defineProperty(SubDV.prototype, 'constructor', { configurable: true, get() { console.log('receiver', this === accessor); return Replacement; } });
console.log('accessor', accessor.constructor === Replacement);
const descriptor = Object.getOwnPropertyDescriptor(DataView.prototype, 'constructor')!;
Object.defineProperty(DataView.prototype, 'constructor', { value: Replacement, configurable: true, writable: true });
console.log('intrinsic-patch', new DataView(new ArrayBuffer(4)).constructor === Replacement);
Object.defineProperty(DataView.prototype, 'constructor', descriptor);
"#).unwrap();
    let node = successful(
        Command::new("node").current_dir(&root).arg(&entry),
        "Node oracle",
    );
    let transcript = String::from_utf8_lossy(&node.stdout);
    assert!(transcript.contains("sub SubDV true true\n"));
    assert!(transcript.contains("null-proto undefined\n"));
    assert!(transcript.contains("receiver true\n"));
    assert!(
        !transcript.contains("false"),
        "all constructor identities must match: {transcript}"
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
        "DataView constructor lookup must follow Node's prototype semantics"
    );
}
