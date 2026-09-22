//! Direct calls through class getters invoke the value returned by the getter.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn direct_calls_through_instance_and_static_getters() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let binary = dir.path().join("main_bin");
    std::fs::write(
        &entry,
        r#"
let reads = 0;
class C {
  get g() { reads++; return (n: number) => n + 2; }
}
const c = new C();
console.log("instance", c.g(1), reads);
const instanceFn = c.g;
console.log("read then call", instanceFn(1), reads);

function make() {
  const cache = new Map<any, any>();
  return class {
    static get g() {
      if (!cache.has(this)) cache.set(this, (n: number) => n + 8);
      return cache.get(this);
    }
  };
}
class G extends make() {}
class H extends G {}
const staticFn = G.g;
console.log("static read", staticFn(1));
console.log("static direct", G.g(1), H.g(2));
"#,
    )
    .expect("write fixture");

    let compile = Command::new(PathBuf::from(env!("CARGO_BIN_EXE_perry")))
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&binary)
        .arg("--no-cache")
        .output()
        .expect("compile fixture");
    assert!(
        compile.status.success(),
        "compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(binary)
        .current_dir(dir.path())
        .output()
        .expect("run fixture");
    assert!(
        run.status.success(),
        "fixture failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        "instance 3 1\nread then call 3 2\nstatic read 9\nstatic direct 9 10\n"
    );
}
