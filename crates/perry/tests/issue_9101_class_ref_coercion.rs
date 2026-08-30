//! Regression for #9101: constructor ClassRefs use an INT32 NaN-box even
//! though they are Function objects. Every coercion path must still run the
//! class's `Symbol.toPrimitive` / `valueOf` / `toString` hooks.

use std::path::PathBuf;
use std::process::{Command, Output};

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(source: &str) -> Output {
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
        .arg("--no-auto-optimize")
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );
    Command::new(&output).output().expect("run compiled binary")
}

#[test]
fn class_refs_run_the_complete_to_primitive_sequence() {
    let run = compile_and_run(
        r#"
class WithToString { static toString() { return "CUSTOM"; } }
let s = "";
for (let i = 0; i < 3; i++) s += WithToString;
console.log(s);

class WithValueOf { static valueOf() { return 42; } }
class WithBoth { static valueOf() { return 7; } static toString() { return "TS"; } }
console.log(WithValueOf * 2, WithBoth - 1, +WithValueOf);

class WithPrimitive {
  static [Symbol.toPrimitive](hint: string) {
    return hint === "number" ? 99 : "PRIM";
  }
  static valueOf() { return -1; }
  static toString() { return "WRONG"; }
}
console.log(WithPrimitive + "", WithPrimitive * 2, String(WithPrimitive));
console.log(typeof WithPrimitive[Symbol.toPrimitive]);

const Expr: any = class {
  static [Symbol.toPrimitive](hint: string) {
    return hint === "number" ? 8 : "EXPR";
  }
};
console.log(Expr - 3, "" + Expr);
"#,
    );
    assert!(
        run.status.success(),
        "compiled program failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout),
        concat!(
            "CUSTOMCUSTOMCUSTOM\n",
            "84 6 42\n",
            "PRIM 198 PRIM\n",
            "function\n",
            "5 EXPR\n",
        )
    );
}
