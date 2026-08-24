//! Runtime regression for forwarded arrays in fallback-free checked-reader
//! loops. Array growth preserves JavaScript identity by leaving a forwarding
//! stub behind; loop admission must normalize one edge to the live array, and
//! a later callback-driven growth must still side-exit before the next effect.

use std::path::PathBuf;
use std::process::{Command, Output};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn run_fixture(binary: &std::path::Path, force_evacuation: bool) -> Output {
    let mut command = Command::new(binary);
    if force_evacuation {
        command.env("PERRY_GC_FORCE_EVACUATE", "1");
    } else {
        command.env_remove("PERRY_GC_FORCE_EVACUATE");
    }
    command
        .output()
        .expect("run versioned indexed-loop fixture")
}

#[test]
fn forwarded_arrays_enter_safely_and_callback_growth_resumes_generically() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let binary = dir.path().join("main_bin");
    std::fs::write(
        &entry,
        r#"
class Reader {
  entities: number[] = [];

  private checkedRead(column: any[] | undefined, index: number, type: number): any {
    if (column === undefined) throw new Error("missing column " + type);
    const value = column[index];
    if (value === 99) throw new Error("missing value " + type + " at " + index);
    return value;
  }

  iterate(
    column: any[],
    callback: (entity: number, value: any) => void,
    entityFilter?: (entity: number) => boolean,
  ): void {
    const entities = this.entities;
    const entityCount = entities.length;
    const cb = callback;
    for (let i = 0; i < entityCount; i++) {
      const entity = entities[i]!;
      if (entityFilter && !entityFilter(entity)) continue;
      cb(entity, this.checkedRead(column, i, 1));
    }
  }
}

const reader = new Reader();
const column: any[] = [];
for (let i = 0; i < 4096; i++) {
  reader.entities.push(i);
  column.push({ n: i });
}

let sum = 0;
let grew = false;
reader.iterate(column, (entity, value) => {
  sum += entity + value.n;
  if (!grew) {
    grew = true;
    for (let i = 0; i < 4096; i++) column.push({ n: -1 });
  }
}, undefined);

console.log(sum + ":" + reader.entities.length + ":" + column.length);
"#,
    )
    .expect("write versioned indexed-loop fixture");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&binary)
        .arg("--no-cache")
        .arg("--no-auto-optimize")
        .output()
        .expect("compile versioned indexed-loop fixture");
    assert!(
        compile.status.success(),
        "compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    for force_evacuation in [false, true] {
        let run = run_fixture(&binary, force_evacuation);
        assert!(
            run.status.success(),
            "fixture failed (force_evacuation={force_evacuation})\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&run.stdout),
            String::from_utf8_lossy(&run.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&run.stdout), "16773120:4096:8192\n");
    }
}
