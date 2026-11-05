//! On wasm32 the class sites take the same lowering as on LP64: a class
//! getter read is the named-accessor call behind its shape guard, and a read
//! of a field the class assigns in its constructor without declaring it is
//! the case-2c family arm (a declared field takes the typed field load on
//! every target). Each test compiles a small program with `--target wasi`
//! and reads the site marker (the block label) from the saved IR; a target
//! refusal leaves it out.
#![cfg(all(unix, feature = "target-wasi"))]

use std::process::Command;

/// The saved IR of `source` compiled for WASI, compile only.
fn wasi_ir(source: &str) -> String {
    let work = tempfile::tempdir().unwrap();
    std::fs::write(work.path().join("main.ts"), source).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_perry"))
        .current_dir(work.path())
        .args([
            "compile",
            "main.ts",
            "--target",
            "wasi",
            "--no-link",
            "--no-auto-optimize",
            "--no-cache",
            "--trace",
            "llvm",
            "-o",
            "main.o",
        ])
        .output()
        .expect("run compile-only WASI");
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let mut ir = String::new();
    for entry in std::fs::read_dir(work.path().join(".perry-trace/llvm")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|x| x == "ll") {
            ir.push_str(&std::fs::read_to_string(path).unwrap());
        }
    }
    assert!(!ir.is_empty(), "the compile saved no IR");
    ir
}

fn labels(ir: &str, label: &str) -> usize {
    ir.lines()
        .filter(|line| line.starts_with(label) && line.ends_with(':'))
        .count()
}

#[test]
fn a_class_getter_read_is_the_named_accessor_call_on_wasm32() {
    let ir = wasi_ir(
        r#"class P {
  x: number; y: number
  constructor(x: number, y: number) { this.x = x; this.y = y }
  get sum(): number { return this.x + this.y }
}
function total(ps: P[]): number { let a = 0; for (let i = 0; i < ps.length; i++) { const p: P = ps[i]; a += p.sum } return a }
const ps: P[] = []
for (let i = 0; i < 10; i++) ps.push(new P(i, i + 1))
console.log(total(ps))
"#,
    );
    assert!(
        labels(&ir, "named_accessor.hit") > 0,
        "the getter read `p.sum` takes no named-accessor site on wasm32"
    );
    assert!(
        ir.contains("@perry_shadow_name_descriptor"),
        "the accessor guard reads shadow bits the entry module never names"
    );
}

#[test]
fn a_class_family_field_read_takes_the_family_arm_on_wasm32() {
    let ir = wasi_ir(
        r#""use strict";
class FamilyFields {
    constructor(a: any, b: any) {
        this.a = a;
        this.b = b;
    }
    read(n: number): any {
        if (n) return this.read(n - 1);
        return this.a + ":" + this.b;
    }
}
class FamilyChild extends FamilyFields {
    constructor(a: any, b: any, c: any) {
        super(a, b);
        this.c = c;
    }
}
function a(o: FamilyFields, n: number): any {
    if (n) return a(o, n - 1);
    return o.a;
}
const one = new FamilyFields(11, 22);
const two = new FamilyChild(1, 2, 3);
console.log(a(one, 1), a(two, 1));
"#,
    );
    assert!(
        labels(&ir, "class_family.word") > 0,
        "the reads of `this.a`/`this.b` in a recursive method take no case-2c family arm on wasm32"
    );
}
