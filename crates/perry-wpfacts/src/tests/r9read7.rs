// R9READ7 (2026-11-16): a per-kind verdict decides each receiver by the
// single-kind proof of that receiver (the same functions: native_member /
// own_length / indexed_length, instance_absent), never a weaker copy.
use super::*;
use perry_hir::Module;

fn hirs(src: &str) -> Vec<(String, Module)> {
    let path = "a.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "a", &path).expect("lowers");
    vec![(path, hir)]
}

fn facts(src: &str) -> crate::ProgramFacts {
    let h = hirs(src);
    let list: Vec<(String, &Module)> = h.iter().map(|(p, m)| (p.clone(), m)).collect();
    let cfg = FactsConfig {
        unit_io: true,
        ..Default::default()
    };
    crate::compute(&list, &cfg)
}

fn reads(pf: &crate::ProgramFacts, key: &str) -> Vec<String> {
    let mi = pf
        .module_paths
        .iter()
        .position(|p| p == "a.ts")
        .expect("a.ts") as u32;
    let v: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.module == mi && s.key == key && !s.access.is_store())
        .map(|s| format!("{:?} {:?}", s.verdict, s.receivers))
        .collect();
    for verdict in &v {
        eprintln!("RR7 .{key} verdict={verdict}");
    }
    v
}

fn proven(src: &str, key: &str) {
    let pf = facts(src);
    let v = reads(&pf, key);
    assert!(
        !v.is_empty() && v.iter().all(|x| !x.starts_with("No(")),
        "{v:?}\n{}",
        pf.dump_text()
    );
}

fn not_proven(src: &str, key: &str) {
    let pf = facts(src);
    let v = reads(&pf, key);
    assert!(v.iter().any(|x| x.starts_with("No(")), "{v:?}");
}

/// `sl` calls `slice` on an Array literal (the intrinsic) and a class
/// instance whose chain lacks it (TypeError); `ln` reads `length` of the
/// Array (own), a typed array (%TypedArray%.prototype getter) and the
/// instance (absent: `undefined`). A typed array method call is never a
/// described member (`native_described`: reads only).
const S: &str = "
class V {
  x: number;
  constructor(x = 0) { this.x = x; }
}
function sl(p: any) { return p.slice(); }
function ln(p: any) { return p.length; }
const arr: any = [1, 2, String(Math.random()).length];
const ta: any = new Float32Array(Number(process.argv.length));
const v: any = new V(3);
/*EXTRA*/
console.log(sl(arr), sl(v), ln(arr), ln(ta), ln(v));
";

fn s(extra: &str) -> String {
    S.replace("/*EXTRA*/", extra)
}

// g1: each receiver decided by its own kind's proof.
#[test]
fn r9read7_g1_array_instance_slice() {
    proven(&s(""), "slice");
}

// KEEP a1 (native arm, Array): an own `slice` stored on the array literal:
// native_member refuses the token alone (native_own_free), so does PerKind.
#[test]
fn r9read7_a1_array_own_store() {
    not_proven(&s("arr.slice = function () { return 0; };"), "slice");
}

// g2: `length` over an Array, a typed array and an instance lacking it.
#[test]
fn r9read7_g2_array_typed_instance_length() {
    proven(&s(""), "length");
}

// KEEP a2 (native arm, TypedArray): an own data `length` defined on the
// typed array: indexed_length refuses the token alone, so does PerKind.
#[test]
fn r9read7_a2_typed_own_length() {
    not_proven(
        &s("Object.defineProperty(ta, \"length\", { value: 7 });"),
        "length",
    );
}

// KEEP a3 (native arm): %TypedArray.prototype% gets a new `length` getter.
#[test]
fn r9read7_a3_typed_proto_written() {
    not_proven(
        &s("Object.defineProperty(Object.getPrototypeOf(Float32Array.prototype), \"length\", { get() { return 1; } });"),
        "length",
    );
}

// KEEP b1 (instance TypeError arm): an unknown computed store onto the
// instance may give it `slice`: instance_absent refuses.
#[test]
fn r9read7_b1_instance_computed_store() {
    not_proven(
        &s("v[process.argv[2] ?? \"q\"] = function () { return 0; };"),
        "slice",
    );
}

// KEEP b2 (instance TypeError arm): a named store of `slice` after birth.
// R9READ11: the instance arm is then "own data if stored, else `undefined`"
// (the stored function or the TypeError arm): decided per kind, never a
// claim codegen uses (T0Mono / T0Set / NativeMember).
#[test]
fn r9read7_b2_instance_named_store() {
    let pf = facts(&s(
        "if (Math.random() < 0.5) v.slice = function () { return 0; };",
    ));
    let v = reads(&pf, "slice");
    assert!(
        !v.is_empty()
            && v.iter()
                .all(|x| x.starts_with("No(") || x.starts_with("PerKind")),
        "{v:?}"
    );
}
