//! Compiling the same program twice emits the same LLVM IR.
//!
//! The private generator frame hands out fresh local ids to the bindings of an
//! inlined resume body. The bindings were collected into a hash set and numbered
//! in that set's iteration order, which is seeded per process, so the same source
//! numbered its locals differently from one compile to the next. The numbers
//! order the capture slots of the frame, so slot offsets, the emitted IR and
//! with it the register allocation of the user code changed run to run
//! (43-generators ran 1.71 G or 2.43 G cycles, depending on the compile).
//!
//! Each compile is a separate perry process, hence a separate hash seed. A
//! program with a few generators and many bindings is compiled several times;
//! the saved `.ll` of every run must be identical.

use std::path::{Path, PathBuf};
use std::process::Command;

const RUNS: usize = 6;

const MAIN: &str = r#"
function* range(start: number, end: number, step: number): Generator<number> {
  for (let i: number = start; i < end; i += step) yield i;
}

function* mapGen(it: Generator<number>, f: (x: number) => number): Generator<number> {
  for (const v of it) yield f(v);
}

function* pairs(n: number): Generator<number> {
  let a = 0;
  let b = 1;
  let flag = false;
  for (let k = 0; k < n; k++) {
    const next = a + b;
    a = b;
    b = next;
    flag = !flag;
    if (flag) yield a;
    else yield b;
  }
}

let total = 0;
for (let r = 0; r < 3; r++) {
  for (const v of mapGen(range(0, 20, 1), (x: number): number => x % 13)) total += v;
  for (const p of pairs(10)) total += p;
}
console.log("RESULT " + total);
"#;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_ir(dir: &Path, run: usize) -> String {
    let src = dir.join("main.ts");
    let ll = dir.join(format!("ll{run}"));
    std::fs::create_dir_all(&ll).unwrap();
    let out = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&src)
        .arg("-o")
        .arg(dir.join(format!("bin{run}")))
        .arg("--no-cache")
        .env("PERRY_SAVE_LL", &ll)
        .output()
        .expect("spawn perry");
    assert!(
        out.status.success(),
        "compile failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mut files: Vec<_> = std::fs::read_dir(&ll)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "ll"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no .ll saved under {}", ll.display());
    files
        .iter()
        .map(|p| format!("; {}\n{}", p.file_name().unwrap().to_string_lossy(), std::fs::read_to_string(p).unwrap()))
        .collect()
}

#[test]
fn the_same_source_compiles_to_the_same_ir() {
    let dir = std::env::temp_dir().join(format!("perry-determinism-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.ts"), MAIN).unwrap();
    let first = compile_ir(&dir, 0);
    assert!(
        first.contains("perry_resume_frame"),
        "fixture no longer reaches the private generator frame"
    );
    for run in 1..RUNS {
        let again = compile_ir(&dir, run);
        if again != first {
            let at = first
                .lines()
                .zip(again.lines())
                .position(|(a, b)| a != b)
                .unwrap_or(0);
            panic!(
                "compile {run} emitted different IR than compile 0 (first difference at line {at}): {}",
                dir.display()
            );
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
