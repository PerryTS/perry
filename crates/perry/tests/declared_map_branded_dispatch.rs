//! Executable semantics for guarded `Map.get` / `ReadonlyMap.get` dispatch.
//! Native Maps bypass generic method lookup while structural values and Map
//! subclasses retain ordinary JavaScript behavior.

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
fn native_structural_subclass_and_nullish_receivers_keep_get_semantics() {
    let stdout = compile_and_run(
        r#"
interface Context {
  values: ReadonlyMap<number, string>;
}

class Holder {
  constructor(public readonly ctx: Context) {}
  lookup(key: number): string | undefined {
    return this.ctx.values.get(key);
  }
}

const native = new Holder({ values: new Map([[2, "two"]]) });
console.log("native", native.lookup(2), native.lookup(3));

let customCalls = 0;
const structural = {
  get(key: number) {
    customCalls++;
    return key === 7 ? "seven" : undefined;
  },
} as unknown as ReadonlyMap<number, string>;
const custom = new Holder({ values: structural });
console.log("structural", custom.lookup(7), custom.lookup(8), customCalls);

class OddMap extends Map<number, string> {
  override get(key: number): string | undefined {
    return key === 99 ? "override" : undefined;
  }
}
const subclass = new Holder({ values: new OddMap([[1, "one"]]) });
console.log("subclass", subclass.lookup(99), subclass.lookup(1));

let nullishRejected = false;
try {
  new Holder({ values: undefined as unknown as ReadonlyMap<number, string> }).lookup(1);
} catch (_error) {
  nullishRejected = true;
}
console.log("nullish", nullishRejected);
"#,
    );

    assert_eq!(
        stdout,
        "native two undefined\n\
         structural seven undefined 2\n\
         subclass override undefined\n\
         nullish true\n"
    );
}
