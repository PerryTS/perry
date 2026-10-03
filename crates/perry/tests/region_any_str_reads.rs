//! Loop regions over fields that are not Number fields (#11680's matrix
//! regressions): an `any` field read as a Number, a string field read through
//! `.length`, and a method call on a receiver whose exact class is proven.
//!
//! Each program is compiled with `--trace llvm`; the test checks the emitted
//! IR of `run` and the program's output (the expected strings are node's).
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

struct TestDir(PathBuf);
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `(IR of every `run` function, stdout)`.
fn compile(source: &str) -> (String, String) {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = TestDir(std::env::temp_dir().join(format!(
        "perry-region-any-str-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )));
    std::fs::create_dir_all(&dir.0).unwrap();
    let entry = dir.0.join("main.ts");
    let binary = dir.0.join("main_bin");
    std::fs::write(&entry, source).unwrap();
    let build = Command::new(env!("CARGO_BIN_EXE_perry"))
        .current_dir(&dir.0)
        .args([
            "compile",
            "--no-auto-optimize",
            "--no-cache",
            "--trace",
            "llvm",
        ])
        .arg(&entry)
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "compile failed: {}",
        String::from_utf8_lossy(&build.stderr)
    );
    let ir = std::fs::read_to_string(dir.0.join(".perry-trace/llvm/main_ts.ll")).unwrap();
    let run = Command::new(&binary).output().unwrap();
    assert!(
        run.status.success(),
        "execution failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    let mut bodies = String::new();
    let mut inside = false;
    for line in ir.lines() {
        if line.starts_with("define ") {
            inside = line.contains("_ts__run");
        }
        if inside {
            bodies.push_str(line);
            bodies.push('\n');
            if line == "}" {
                inside = false;
            }
        }
    }
    assert!(!bodies.is_empty(), "no `run` function in the IR");
    (bodies, String::from_utf8(run.stdout).unwrap())
}

/// The region R mask each learned prime call requests (its last argument).
fn prime_r_masks(ir: &str) -> Vec<u32> {
    ir.lines()
        .filter(|line| line.contains("@js_region_loop_prime("))
        .filter_map(|line| {
            line.rsplit_once("i32 ")?
                .1
                .split_once(')')?
                .0
                .trim()
                .parse()
                .ok()
        })
        .collect()
}

/// The bodies of the guard's value-test blocks (`rloop.guard.value.N:`).
fn value_test_blocks(ir: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur: Option<String> = None;
    for line in ir.lines() {
        if !line.starts_with(' ') && line.ends_with(':') {
            if let Some(block) = cur.take() {
                out.push(block);
            }
            let label = line.trim_end_matches(':');
            let tail = label.strip_prefix("rloop.guard.value.");
            if tail.is_some_and(|t| t.chars().all(|c| c.is_ascii_digit())) {
                cur = Some(String::new());
            }
            continue;
        }
        if let Some(block) = cur.as_mut() {
            block.push_str(line);
            block.push('\n');
        }
    }
    out.extend(cur);
    out
}

const ANY_CLASS: &str = r#"
class C {
    a: any;
    d: any;
    constructor(a: any, d: any) { this.a = a; this.d = d; }
}
function run(n: number, o: any): any {
    let h = 0.0;
    for (let k = 0; k < n; k++) {
        o.d = k;
        h += o.a;
    }
    return h;
}
"#;

/// An `any` field read as a Number keeps its region: the learned word asks
/// for R, and the guard serves it with a value test of the slot, so a Number
/// runs F and a string runs G, both with node's answer.
#[test]
fn an_any_field_number_read_is_served_by_a_guard_value_test() {
    let (ir, stdout) = compile(&format!(
        "{ANY_CLASS}
const o = new C(2, 0);
console.log(run(10, o));
o.a = \"s\";
console.log(run(3, o));
o.a = 1.5;
console.log(run(4, o), o.d);
"
    ));
    assert_eq!(stdout, "20\n0sss\n6 3\n");
    let masks = prime_r_masks(&ir);
    assert!(
        !masks.is_empty() && masks.iter().all(|&m| m != 0),
        "the region must request R for `o.a`: {masks:?}"
    );
    let tests: Vec<String> = value_test_blocks(&ir);
    assert!(
        tests
            .iter()
            .any(|block| block.contains("load double") && block.contains("9221401712017801216")),
        "the guard must load each R slot and compare it below the tag band:\n{tests:#?}\n{ir}"
    );
}

/// A string field read through `.length` is a receiver, never a Number
/// operand: the region asks for no Number lane and emits no value test.
#[test]
fn a_string_field_length_read_requests_no_number_lane() {
    let (ir, stdout) = compile(
        r#"
function run(n: number, o: any): number {
    let h = 0;
    for (let k = 0; k < n; k++) {
        o.d = k;
        h += o.a.length;
    }
    return h;
}
console.log(run(5, { a: "abc", d: "dd" }));
"#,
    );
    assert_eq!(stdout, "15\n");
    let masks = prime_r_masks(&ir);
    assert!(
        !masks.is_empty(),
        "the region must form (it serves `o.d` and `o.a`):\n{ir}"
    );
    assert!(
        masks.iter().all(|&m| m == 0),
        "`o.a` is `.length`'s receiver, not a Number operand: {masks:?}"
    );
    assert!(!ir.contains("rloop.guard.value"), "no R, so no value test");
}

/// A Number field on an identity F64 lane pays no value test: the static
/// word of a literal names its lanes, and the guard emits none.
#[test]
fn a_number_field_read_pays_no_value_test() {
    let (ir, stdout) = compile(
        r#"
const SINK: any[] = [];
function run(n: number): number {
    const o: any = { a: 1, d: 16 };
    let h = 0.0;
    for (let k = 0; k < n; k++) {
        o.d = k;
        h += o.a;
    }
    SINK.push(o);
    return h + o.d;
}
console.log(run(10));
"#,
    );
    assert_eq!(stdout, "19\n");
    assert!(ir.contains("rloop.fast"), "the region must form:\n{ir}");
    assert!(
        !ir.contains("rloop.guard.value"),
        "an F64 lane needs no value test:\n{ir}"
    );
}

/// A method call on a receiver whose exact class is proven (Ptr<Shape>)
/// stays the guard-free direct call inside an admitted region: the region
/// owns its receivers' slot accesses, not a call's dispatch.
#[test]
fn a_proven_receiver_method_call_stays_guard_free_inside_a_region() {
    let (ir, stdout) = compile(
        r#"
class C {
    a: number;
    d: number;
    constructor(a: number, d: number) { this.a = a; this.d = d; }
    m() { return this.a; }
}
function run(n: number): any {
    const o: any = new C(3, 0);
    let h = 0.0;
    for (let k = 0; k < n; k++) {
        o.d = k;
        h += o.m();
    }
    if (h < 0) return o;
    return h + o.d;
}
console.log(run(10));
"#,
    );
    assert_eq!(stdout, "39\n");
    assert!(ir.contains("rloop.fast"), "the region must form:\n{ir}");
    assert!(
        !ir.contains("@js_typed_feedback_method_direct_call_guard("),
        "the proven receiver's call must not take the guarded dispatch:\n{ir}"
    );
}
