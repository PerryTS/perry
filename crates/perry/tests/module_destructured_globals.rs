//! Regression tests for #6649 (pi wall #4): module-scope ARRAY-destructured
//! bindings read from function bodies.
//!
//! The array-pattern lowering wraps each leaf `Stmt::Let` in the
//! iterator-protocol `Stmt::Try` scaffolding (IteratorClose on abrupt
//! completion), and codegen's module-global promotion pre-walk only scanned
//! TOP-LEVEL init statements — so array-destructured leaves were never
//! globalized and every function/method/closure reference compiled to the
//! not-in-scope fallback (`undefined`). Object-pattern leaves (emitted at
//! statement level) were unaffected. First loud symptom: TypeBox's FNV-1a
//! hash module (`var [Prime, Size] = [BigInt(...), BigInt(...)]`) threw a
//! spurious "Cannot mix BigInt and other types, use explicit conversions"
//! during pi-bundle init (`Accumulator * Prime` with `Prime === undefined`
//! → `ToNumeric(undefined) = NaN`), while node ran the identical bundle
//! clean.

use std::path::PathBuf;
use std::process::Command;

fn perry_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_perry"))
}

fn compile_and_run(dir: &std::path::Path, source: &str) -> String {
    let entry = dir.join("main.ts");
    let output = dir.join("main_bin");
    std::fs::write(&entry, source).expect("write entry");

    let compile = Command::new(perry_bin())
        .current_dir(dir)
        .arg("compile")
        .arg(&entry)
        .arg("-o")
        .arg(&output)
        .output()
        .expect("run perry compile");
    assert!(
        compile.status.success(),
        "perry compile failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&output)
        .current_dir(dir)
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

/// The TypeBox hash-module shape that killed pi-native init, plus every
/// destructuring form (nested, default, rest, object-in-array, let/const/var)
/// across the primitive families, read from a function, an arrow, an instance
/// method, and a static method — and written back from a function.
#[test]
fn module_array_destructured_bindings_visible_from_functions() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
var Accumulator = BigInt("14695981039346656037");
var [Prime, Size] = [BigInt("1099511628211"), BigInt("18446744073709551616")];
var Bytes = Array.from({ length: 256 }).map((_x, i) => BigInt(i));
function FNV1A64_OP(byte: number) {
  Accumulator = Accumulator ^ Bytes[byte];
  Accumulator = Accumulator * Prime % Size;
}
function HashCode() {
  Accumulator = BigInt("14695981039346656037");
  FNV1A64_OP(8);
  return Accumulator;
}
console.log("fnv:", HashCode().toString(16).padStart(16, "0"));

var [S1] = ["hello"];
var [N1] = [42.5];
const [C1] = ["c-const"];
let [L1] = ["l-let"];
var [A1, [A2]] = ["a1", ["a2"]];
var [D1 = "dflt"] = [];
var [P1, ...P2] = ["p1", "p2", "p3"];
var [{ M1 }] = [{ M1: "mixed" }];
function readAll(): string {
  return [typeof S1, S1, N1, C1, L1, A1, A2, D1, P1, P2.join(","), M1].join("|");
}
console.log("fn:", readAll());
console.log("arrow:", (() => `${S1}/${A2}/${D1}`)());
class Reader {
  read(): string { return `${N1}:${C1}`; }
  static sread(): string { return `${L1}:${P2.length}`; }
}
console.log("method:", new Reader().read(), Reader.sread());
function bump() { L1 = L1 + "!"; N1 = N1 + 1; }
bump();
console.log("bumped:", L1, N1);
"#,
    );
    assert_eq!(
        stdout,
        "fnv: af63c54c8601c577\n\
         fn: string|hello|42.5|c-const|l-let|a1|a2|dflt|p1|p2,p3|mixed\n\
         arrow: hello/a2/dflt\n\
         method: 42.5:c-const l-let:2\n\
         bumped: l-let! 43.5\n"
    );
}

/// `>>>` with exactly one BigInt operand throws Node's MIXED-operand
/// TypeError; the dedicated "no unsigned right shift" TypeError is reserved
/// for the both-BigInt case (the spec's both-BigInt type check precedes the
/// operator lookup). Pre-fix, perry threw the no-unsigned-shift message for
/// any BigInt operand.
#[test]
fn ushr_mixed_bigint_message_matches_node() {
    let dir = tempfile::tempdir().expect("tempdir");
    let stdout = compile_and_run(
        dir.path(),
        r#"
function probe(label: string, fn: () => unknown): void {
  try {
    console.log(label, "ok", String(fn()));
  } catch (e) {
    const err = e as Error;
    console.log(label, "throw", err.name, err.message);
  }
}
const vals: any[] = [1n, 2, "3", 3n];
const big: any = vals[0];
const num: any = vals[1];
const str: any = vals[2];
const big2: any = vals[3];
probe("ushr big num", () => big >>> num);
probe("ushr num big", () => num >>> big);
probe("ushr big str", () => big >>> str);
probe("ushr big big", () => big >>> big2);
probe("ushr num num", () => num >>> 1);
"#,
    );
    assert_eq!(
        stdout,
        "ushr big num throw TypeError Cannot mix BigInt and other types, use explicit conversions\n\
         ushr num big throw TypeError Cannot mix BigInt and other types, use explicit conversions\n\
         ushr big str throw TypeError Cannot mix BigInt and other types, use explicit conversions\n\
         ushr big big throw TypeError BigInts have no unsigned right shift, use >> instead\n\
         ushr num num ok 1\n"
    );
}
