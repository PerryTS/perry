//! Executable semantics for the `ReadonlySet.has` branded fast path.
//! Native Sets bypass generic dispatch, while TypeScript's structural values
//! and Set subclasses retain ordinary JavaScript method lookup.

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
fn native_structural_and_subclass_receivers_keep_has_semantics() {
    let stdout = compile_and_run(
        r#"
class Holder {
  constructor(public readonly values: ReadonlySet<number>) {}
  contains(value: number): boolean {
    return this.values.has(value);
  }
}

function nullableContains(holder: Holder | undefined, value: number): boolean {
  return holder.values.has(value);
}

const native = new Holder(new Set([2, 4, 6]));
console.log("native", native.contains(4), native.contains(5));

let customCalls = 0;
const structural = {
  has(value: number) {
    customCalls++;
    return value === 7;
  },
} as unknown as ReadonlySet<number>;
const custom = new Holder(structural);
console.log("structural", custom.contains(7), custom.contains(8), customCalls);
console.log("nullable", nullableContains(custom, 7), customCalls);

let nullishRejected = false;
try {
  nullableContains(undefined, 7);
} catch (_error) {
  nullishRejected = true;
}
console.log("nullish", nullishRejected);

class OddSet extends Set<number> {
  override has(value: number): boolean {
    return value === 99;
  }
}
const subclass = new Holder(new OddSet([1, 3]));
console.log("subclass", subclass.contains(99), subclass.contains(1));
"#,
    );

    assert_eq!(
        stdout,
        "native true false\n\
         structural true false 2\n\
         nullable true 3\n\
         nullish true\n\
         subclass true false\n"
    );
}
