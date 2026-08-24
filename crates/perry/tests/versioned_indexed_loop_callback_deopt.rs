//! Runtime regression for the zero-steady-state-check callback specialization
//! in versioned checked-reader loops. Cold property/addition arms must mark the
//! current loop for an exact once-only resume before any observable fallback.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Once;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize workspace root")
}

fn runtime_dir() -> PathBuf {
    static BUILD_RUNTIME: Once = Once::new();
    BUILD_RUNTIME.call_once(|| {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let mut command = Command::new(cargo);
        command.current_dir(workspace_root()).arg("build");
        if !cfg!(debug_assertions) {
            command.arg("--release");
        }
        let build = command
            .args(["-p", "perry-runtime-static"])
            .output()
            .expect("build static runtime archive");
        assert!(
            build.status.success(),
            "static runtime build failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&build.stdout),
            String::from_utf8_lossy(&build.stderr)
        );
    });

    perry_bin()
        .parent()
        .expect("Perry binary directory")
        .to_path_buf()
}

fn run_fixture(binary: &Path, force_evacuation: bool) -> Output {
    let mut command = Command::new(binary);
    if force_evacuation {
        command
            .env("PERRY_GC_FORCE_EVACUATE", "1")
            .env("PERRY_GC_VERIFY_EVACUATION", "1");
    } else {
        command
            .env_remove("PERRY_GC_FORCE_EVACUATE")
            .env_remove("PERRY_GC_VERIFY_EVACUATION");
    }
    command.output().expect("run callback-deopt fixture")
}

fn llvm_function_body(ir: &str, symbol: &str) -> String {
    let start = ir
        .lines()
        .position(|line| line.starts_with("define") && line.contains(symbol))
        .unwrap_or_else(|| panic!("no LLVM definition containing {symbol:?}"));
    ir.lines()
        .skip(start)
        .take_while(|line| *line != "}")
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn cold_callback_arms_resume_once_at_the_next_index() {
    let dir = tempfile::tempdir().expect("tempdir");
    let entry = dir.path().join("main.ts");
    let binary = dir.path().join("main_bin");
    std::fs::write(
        &entry,
        r#"
class Reader {
  entities: number[] = [];

  private checkedRead(column: any[], index: number, type: number): any {
    if (column === undefined) throw new Error("missing column " + type);
    const value = column[index];
    if (value === undefined) throw new Error("missing value " + type);
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

  iterateCaught(
    column: any[],
    callback: (entity: number, value: any) => void,
    entityFilter?: (entity: number) => boolean,
  ): string {
    const entities = this.entities;
    const entityCount = entities.length;
    const cb = callback;
    try {
      for (let i = 0; i < entityCount; i++) {
        const entity = entities[i]!;
        if (entityFilter && !entityFilter(entity)) continue;
        cb(entity, this.checkedRead(column, i, 1));
      }
    } catch (_error) {
      return entities.length + ":" + column.length;
    }
    return "none";
  }
}

function makeReader(count: number): Reader {
  const reader = new Reader();
  for (let i = 0; i < count; i++) reader.entities.push(i);
  return reader;
}

const plainReader = makeReader(4);
let plainSum: any = 0;
plainReader.iterate(
  [{ n: 1 }, { n: 2 }, { n: 3 }, { n: 4 }],
  (_entity, value) => { plainSum += value.n; },
  undefined,
);

const mutatingReader = makeReader(4);
const accessor: any = {};
const mutatingColumn: any[] = [{ n: 10 }, accessor, { n: 30 }, { n: 40 }];
let getterCalls = 0;
Object.defineProperty(accessor, "n", {
  get() {
    getterCalls++;
    mutatingColumn.push({ n: 50 });
    (mutatingReader as any).checkedRead = (
      _column: any[],
      index: number,
      _type: number,
    ) => ({ n: 100 + index });
    return 20;
  },
});
let mutatingSum: any = 0;
mutatingReader.iterate(
  mutatingColumn,
  (_entity, value) => { mutatingSum += value.n; },
  undefined,
);

const stringReader = makeReader(3);
let stringSum: any = 0;
stringReader.iterate(
  [{ n: 1 }, { n: "x" }, { n: 3 }],
  (_entity, value) => { stringSum += value.n; },
  undefined,
);

const caughtReader = makeReader(2);
let caughtSum: any = 0;
const caughtCallback = (_entity: number, value: any) => { caughtSum += value.n; };
caughtReader.iterate([{ n: 1 }, { n: 2 }], caughtCallback, undefined);
caughtSum = 0;
const throwingValue: any = {};
Object.defineProperty(throwingValue, "n", {
  get() {
    const churn: any[] = [];
    for (let i = 0; i < 2048; i++) churn.push({ i });
    throw new Error("cold getter");
  },
});
const caught = caughtReader.iterateCaught(
  [{ n: 1 }, throwingValue],
  caughtCallback,
  undefined,
);

console.log(
  plainSum + ":" + mutatingSum + ":" + mutatingColumn.length + ":" +
  getterCalls + ":" + stringSum + ":" + caught + ":" + caughtSum,
);
"#,
    )
    .expect("write callback-deopt fixture");

    let compile = Command::new(perry_bin())
        .current_dir(dir.path())
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&binary)
        .arg("--no-cache")
        .arg("--no-auto-optimize")
        .arg("--trace")
        .arg("llvm")
        .env("PERRY_RUNTIME_DIR", runtime_dir())
        .output()
        .expect("compile callback-deopt fixture");
    assert!(
        compile.status.success(),
        "compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let ir = std::fs::read_to_string(dir.path().join(".perry-trace/llvm/main_ts.ll"))
        .expect("read traced main module LLVM IR");
    let ordinary = llvm_function_body(&ir, "__Reader__iterate$undef2(");
    let caught = llvm_function_body(&ir, "__Reader__iterateCaught$undef2(");
    assert!(
        ordinary.contains("versioned_index.loop.callback.preheader"),
        "ordinary loop should select the exact callback version:\n{ordinary}"
    );
    assert!(
        !caught.contains("versioned_index.loop.callback.preheader"),
        "an active local EH scope must keep the collecting callback clone out:\n{caught}"
    );

    for force_evacuation in [false, true] {
        let run = run_fixture(&binary, force_evacuation);
        assert!(
            run.status.success(),
            "fixture failed (force_evacuation={force_evacuation})\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&run.stdout),
            String::from_utf8_lossy(&run.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            "10:235:5:1:1x3:2:2:1\n"
        );
    }
}
