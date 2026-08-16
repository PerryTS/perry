//! Regression coverage for #7841: `s += rhs` may use the string-builder
//! lowering because `s` is declared `string`, but that annotation is erased at
//! runtime and cannot decide whether JavaScript `+` concatenates or adds.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, source: &str, canonical_strings: bool) -> String {
    let entry = dir.join(if canonical_strings {
        "canonical.ts"
    } else {
        "boxed.ts"
    });
    let output = dir.join(if canonical_strings {
        "canonical_bin"
    } else {
        "boxed_bin"
    });
    std::fs::write(&entry, source).expect("write entry");

    let mut compiler = Command::new(perry_bin());
    compiler
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .arg("--no-cache");
    if !canonical_strings {
        compiler.env("PERRY_CANONICAL_STR_LOCALS", "0");
    }
    let compile = compiler.output().expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed (canonical={canonical_strings})\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output)
        .current_dir(dir)
        .output()
        .expect("run compiled binary");
    assert!(
        run.status.success(),
        "compiled binary failed (canonical={canonical_strings})\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    String::from_utf8_lossy(&run.stdout).into_owned()
}

#[test]
fn declared_string_self_append_uses_runtime_values_and_preserves_order() {
    let source = r#"
let numeric: string = (42 as any);
numeric += 1;
console.log("numeric", numeric, typeof numeric);

let valueOfCalls = 0;
let toStringCalls = 0;
const rhs: any = {
  valueOf() { valueOfCalls += 1; return 1; },
  toString() { toStringCalls += 1; return "wrong"; }
};
let dynamic: string = (42 as any);
dynamic += rhs;
console.log("dynamic", dynamic, typeof dynamic, valueOfCalls, toStringCalls);

let stringValueOfCalls = 0;
let stringToStringCalls = 0;
const stringRhs: any = {
  valueOf() { stringValueOfCalls += 1; return 7; },
  toString() { stringToStringCalls += 1; return "wrong"; }
};
let actual: string = "a";
actual += stringRhs;
console.log("actual", actual, stringValueOfCalls, stringToStringCalls);

let declaredLeft: string = (40 as any);
let declaredRight: string = (2 as any);
declaredLeft += declaredRight;
console.log("both-lie", declaredLeft, typeof declaredLeft);

let order: string = "old";
order += (order = "new");
console.log("order", order);

let built: string = "prefix";
for (let i = 0; i < 1000; i++) built += "_chunk";
console.log("builder", built.length);
"#;
    let expected = concat!(
        "numeric 43 number\n",
        "dynamic 43 number 1 0\n",
        "actual a7 1 0\n",
        "both-lie 42 number\n",
        "order oldnew\n",
        "builder 6006\n",
    );

    for canonical in [true, false] {
        let dir = tempfile::tempdir().expect("tempdir");
        let output = compile_and_run(dir.path(), source, canonical);
        assert_eq!(output, expected, "canonical strings = {canonical}");
    }
}
