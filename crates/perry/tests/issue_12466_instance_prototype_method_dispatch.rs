use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(source: &str) -> String {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output)
        .current_dir(dir.path())
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed\nstatus: {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status,
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

#[test]
fn typed_base_array_call_observes_instance_prototype_change() {
    let stdout = compile_and_run(
        r#"
class Base {
  scale: number;
  constructor(n: number) { this.scale = n; }
  step(x: number) { return x * this.scale + 1; }
}
class Derived extends Base { step(x: number) { return x * this.scale + 2; } }
function callAt(xs: Base[], index: number) { return xs[index].step(3); }
const xs: Base[] = [new Base(2), new Derived(4)];
callAt(xs, 0);
Object.setPrototypeOf(xs[0], { step(x: number) { return this.scale + x + 400; } });
console.log('edge', callAt(xs, 0));
"#,
    );
    assert_eq!(stdout, "edge 405\n");
}
