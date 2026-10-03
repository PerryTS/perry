//! #11759 (c′): a class declaration that may be evaluated more than once keeps
//! the shared class for its FIRST evaluation and creates a fresh class object
//! for each later one. The check is one load of the template's flag, a compare
//! and a branch, and it is emitted only where `lower::run_once` cannot prove
//! the definition runs once.
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

const FLAG: &str = "@perry_class_first_eval.";
const FRESH: &str = "call i64 @js_class_evaluation_object(";

struct TestDir(PathBuf);
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn compile(source: &str) -> (String, String) {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = TestDir(std::env::temp_dir().join(format!(
        "perry-11759-{}-{}",
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
    (ir, String::from_utf8(run.stdout).unwrap())
}

/// The lines that use `needle` outside its own definition.
fn uses(ir: &str, needle: &str) -> usize {
    ir.lines()
        .filter(|line| line.contains(needle) && !line.starts_with('@'))
        .count()
}

#[test]
fn a_repeatable_declaration_tests_the_template_flag_and_its_later_evaluations_are_fresh() {
    let (ir, stdout) = compile(
        r#"
function f() { class K { static n = 1; } return K; }
const a: any = f(), b: any = f(), c: any = f();
b.n = 2;
console.log(a === b, b === c, a.n, b.n, c.n, new b() instanceof a, new c() instanceof c);
"#,
    );
    assert_eq!(stdout, "false false 1 2 1 false true\n");
    assert!(
        ir.lines()
            .any(|line| line.starts_with(FLAG) && line.contains("global i64 0")),
        "the template flag is defined once per module:\n{ir}"
    );
    assert!(
        uses(&ir, FLAG) >= 2,
        "the evaluation loads and sets the flag"
    );
    assert!(
        uses(&ir, FRESH) >= 1,
        "a later evaluation is a fresh class object"
    );
}

#[test]
fn a_module_top_loop_body_declaration_is_repeatable() {
    let (ir, stdout) = compile(
        r#"
const cs: any[] = [];
for (let i = 0; i < 3; i++) { class L { static k = i; } cs.push(L); }
console.log(cs[0] === cs[1], cs[1] === cs[2], cs.map((c: any) => c.k).join());
"#,
    );
    assert_eq!(stdout, "false false 0,1,2\n");
    assert!(uses(&ir, FLAG) >= 2, "a loop body is not run once:\n{ir}");
}

#[test]
fn a_definition_proven_to_run_once_emits_no_check() {
    let (ir, stdout) = compile(
        r#"
const once: any = (function () { class O { static tag = "o"; } return O; })();
let blockClass: any;
{ class B { static tag = "b"; } blockClass = B; }
function wrapper() { class W { static tag = "w"; } return W; }
const w: any = wrapper();
console.log(once.tag, new once() instanceof once, blockClass.tag, w.tag);
"#,
    );
    assert_eq!(stdout, "o true b w\n");
    assert_eq!(
        uses(&ir, FLAG),
        0,
        "an IIFE body, a module-top block and a function called once from module top run once:\n{ir}"
    );
    assert!(
        !ir.contains(FLAG),
        "no flag is defined for a run-once class"
    );
    assert_eq!(
        uses(&ir, FRESH),
        0,
        "a run-once class is never a fresh class object"
    );
}

#[test]
fn an_esbuild_lazy_wrapper_keeps_its_one_class() {
    // esbuild's `__esm` arrow runs its body at most once, which only the
    // runtime can see: the flag check is emitted and its first (only)
    // evaluation is the shared class.
    let (ir, stdout) = compile(
        r#"
var __esm = (fn: any, res?: any) => function () {
  return fn && (res = (0, fn[Object.keys(fn)[0]])(fn = 0)), res;
};
let E1: any;
const init_x: any = __esm({ "x.ts"() { class EK { static q = 3; } E1 = EK; } });
init_x();
const e1 = E1;
init_x();
console.log(e1 === E1, E1.q, new E1() instanceof e1);
"#,
    );
    assert_eq!(stdout, "true 3 true\n");
    assert!(
        uses(&ir, FLAG) >= 2,
        "the wrapper body is not provably run once:\n{ir}"
    );
}

/// A loop that cannot rebind the class binding tests the first evaluation
/// once, before the loop, and lowers itself twice: the first-evaluation copy
/// holds the static `new` (which scalar replacement then sees) and the static
/// field read, the later-evaluation copy the by-value forms.
#[test]
fn a_loop_over_a_repeatable_class_tests_its_first_evaluation_once() {
    let (ir, out) = compile(
        r#"
function run(n: number) {
  class C { x: number; constructor(x: number) { this.x = x; } static s = 3; }
  let t = 0;
  for (let i = 0; i < n; i++) { const c = new C(i); t += c.x + C.s; }
  return t;
}
console.log(run(3));
console.log(run(3));
"#,
    );
    assert_eq!(out, "12\n12\n");
    assert!(
        ir.contains("classloop.first"),
        "the loop is versioned:\n{ir}"
    );
    assert!(
        ir.contains("classloop.later"),
        "the loop is versioned:\n{ir}"
    );
}

/// A loop that rebinds the class binding keeps its per-use tests: the
/// binding can stop holding the first evaluation mid-loop.
#[test]
fn a_loop_that_rebinds_the_class_keeps_its_per_use_tests() {
    let (ir, out) = compile(
        r#"
function run(k: number) {
  class C { static s = 1; }
  let r = 0;
  for (let i = 0; i < 2; i++) {
    r += C.s;
    if (k) { (C as any) = class { static s = 10; }; }
  }
  return r;
}
console.log(run(1));
console.log(run(0));
"#,
    );
    assert_eq!(out, "11\n2\n");
    assert!(
        !ir.contains("classloop.first"),
        "a rebinding loop is not versioned:\n{ir}"
    );
}
