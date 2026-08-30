//! Regression coverage for #9123: deleting a declared prototype method must
//! invalidate compiler-emitted direct-method guards.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

#[test]
fn prototype_method_delete_invalidates_direct_dispatch() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let output = dir.path().join("main_bin");
    std::fs::write(
        &entry,
        r#"
class C {
  a = 5;

  inc(): number {
    return this.a + 1;
  }
}

function viaParam(c: C, n: number): number {
  let last = 0;
  for (let i = 0; i < n; i++) {
    last = c.inc();
  }
  return last;
}

delete (C.prototype as any).inc;
try {
  console.log(viaParam(new C(), 1));
} catch (error) {
  console.log("threw:", (error as Error).constructor.name);
}
"#,
    )
    .expect("write source");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache")
        .arg("--no-auto-optimize")
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
        "compiled binary failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), "threw: TypeError\n");
}
