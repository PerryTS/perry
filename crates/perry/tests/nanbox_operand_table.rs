//! The runtime's NaN-box operand table (`PERRY_NANBOX_OPERANDS`) as x86-64
//! generated code uses it (`perry-codegen/src/inprocess/nanbox_operands.rs`).
//!
//! Compiled tag tests read their 64-bit operands from the table instead of
//! `movabs` immediates. The program below classifies every kind of value
//! (the singletons, numbers, strings short and long, objects, arrays,
//! functions, BigInt, symbols, holes) through those tests, so a wrong table
//! entry, or an operand mapped to the wrong entry, changes its output.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

const SOURCE: &str = r#"
const vals: any[] = [undefined, null, true, false, 0, -1.5, NaN, "s", "a longer string", 123456789012, 7, { a: 1 }, [1, 2], () => 1, 10n, Symbol.iterator];
function kind(v: any): string {
  if (v === undefined) return "u";
  if (v === null) return "n";
  if (v === true) return "t";
  if (v === false) return "f";
  return typeof v;
}
let out = "";
for (const v of vals) out += kind(v) + (v == null ? "!" : "") + (v ? "1" : "0") + ",";
console.log(out);
const holes = [1, , 3];
console.log(1 in holes, holes[1] === undefined, holes.length);
let acc = 0;
for (let i = 0; i < 1000; i++) acc = (acc + i * 7) | 0;
console.log(acc);
const o: any = { x: 1, y: "two" };
console.log(o.x + 1, o.y.length, o.z === undefined, JSON.stringify(o));
function sum(xs: any[]): number {
  let s = 0;
  for (const x of xs) if (typeof x === "number") s += x;
  return s;
}
console.log(sum(vals), String(vals[8]).length + vals[7].length);
"#;

const EXPECTED: &str = r#"u!0,n!0,t1,f0,number0,number1,number0,string1,string1,number1,number1,object1,object1,function1,bigint1,symbol1,
false true 3
3496500
2 3 true {"x":1,"y":"two"}
NaN 16
"#;

fn compile(dir: &std::path::Path) -> PathBuf {
    let entry = dir.join("nb_main.ts");
    std::fs::write(&entry, SOURCE).expect("write entry");
    let exe = dir.join("nb_main");
    let out = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&exe)
        .arg("--no-cache")
        .env("PERRY_KEEP_SYMBOLS", "1")
        .output()
        .expect("run perry compile");
    assert!(
        out.status.success(),
        "compile failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    exe
}

#[test]
fn values_classify_as_node_does_through_the_operand_table() {
    let dir = tempfile::tempdir().expect("tempdir");
    let exe = compile(dir.path());
    let out = Command::new(&exe).output().expect("run the program");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success() && stdout == EXPECTED,
        "status {:?}\nstdout:\n{stdout}\nstderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr),
    );
    // Witness that the program above exercised the table at all: the
    // classifier reads its operands from it.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        let nm = Command::new("nm").arg(&exe).output().expect("run nm");
        let symbols = String::from_utf8_lossy(&nm.stdout);
        let kind = symbols
            .lines()
            .filter_map(|l| l.split_whitespace().nth(2))
            .find(|s| s.starts_with("perry_fn_") && s.ends_with("nb_main_ts__kind"))
            .unwrap_or_else(|| panic!("no kind() symbol in:\n{symbols}"))
            .to_string();
        let dis = Command::new("objdump")
            .args(["-d", "--no-show-raw-insn", &format!("--disassemble={kind}")])
            .arg(&exe)
            .output()
            .expect("run objdump");
        let text = String::from_utf8_lossy(&dis.stdout);
        assert!(
            text.contains("<PERRY_NANBOX_OPERANDS"),
            "kind() must read its NaN-box operands from the table:\n{text}"
        );
    }
}
