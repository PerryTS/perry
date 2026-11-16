// R9READ6 (2026-11-16): mixed-kind receivers: every receiver kind decided
// by its own kind proof (class instance, Array, primitive), the op's own
// type dispatch telling them apart (owner 2026-11-13).
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
        .map(|s| format!("{:?}", s.verdict))
        .collect();
    for verdict in &v {
        eprintln!("RR6 .{key} verdict={verdict}");
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

/// Class instances (V, W), an array literal and a Number reach `p`.
const M: &str = "
class V {
  x: number;
  constructor(x = 0) { this.x = x; }
  clone() { return new V(this.x); }
}
V.prototype.isV = true;
class W {
  y: number;
  constructor(y = 0) { this.y = y; }
  clone() { return new W(this.y); }
}
/*EXTRA*/
function rd(p: any) { return p.isV; }
function cl(p: any) { return p.clone(); }
function sl(p: any) { return p.slice(); }
const n = Number(process.argv.length);
const arr = [1, 2, String(Math.random()).length];
console.log(rd(new V(1)), rd(new W(2)), rd(arr), /*RDN*/ 0);
console.log(cl(new V(1)), cl(new W(2)), cl(arr));
console.log(sl(arr), sl(new V(3)), sl([n]));
";

fn m(extra: &str, rdn: &str) -> String {
    M.replace("/*EXTRA*/", extra).replace("/*RDN*/", rdn)
}

// m1: a read over class instances (a prototype constant on V, absent on
// W) beside an Array (absent): each kind its own [[Get]] outcome.
#[test]
fn m1_read_instances_and_array() {
    proven(&m("", ""), "isV");
}

// m2: the same with a Number beside them: ToObject -> %Number.prototype%
// -> %Object.prototype%: `undefined` (6.2.5.5, 7.3.2).
#[test]
fn m2_read_instances_array_number() {
    proven(&m("", "rd(n),"), "isV");
}

// m3: a call over class instances (one body each) beside an Array, whose
// chain lacks `clone`: its TypeError arm (7.3.14).
#[test]
fn m3_call_instances_and_array() {
    proven(&m("", ""), "clone");
}

// m4: `slice` over Arrays (the intrinsic) beside a class instance whose
// chain lacks it (TypeError arm).
#[test]
fn m4_call_array_native_and_instance() {
    proven(&m("", ""), "slice");
}

// KEEP k1: the program writes `isV` onto Array.prototype: the Array arm
// reads it.
#[test]
fn k1_array_proto_written() {
    not_proven(
        &m("(Array.prototype as any).isV = process.argv[2];", ""),
        "isV",
    );
}

// KEEP k2: Number.prototype gains `isV`: the Number arm reads it.
#[test]
fn k2_number_proto_written() {
    not_proven(
        &m("(Number.prototype as any).isV = process.argv[2];", "rd(n),"),
        "isV",
    );
}

// KEEP k3: Object.prototype gains `clone`: the Array arm finds it.
#[test]
fn k3_object_proto_clone_written() {
    not_proven(
        &m(
            "(Object.prototype as any).clone = function () { return 1; };",
            "",
        ),
        "clone",
    );
}

// KEEP k4: an unknown computed store onto W.prototype may add `isV`.
#[test]
fn k4_unknown_key_on_proto() {
    not_proven(&m("(W.prototype as any)[process.argv[2]] = 1;", ""), "isV");
}

/// Rule T6: `typeof local === "string"` narrows the local on the right of
/// `&&`. `params[0]` holds a String, an object literal and a Number.
const T: &str = "
function f(...params: any[]) {
  /*DECL*/
  /*BODY*/
  return 0;
}
f(process.argv[2]);
f({ startsWith: (s: string) => s.length });
f(Number(process.argv.length));
";

fn t(decl: &str, body: &str) -> String {
    T.replace("/*DECL*/", decl).replace("/*BODY*/", body)
}

// t1: a `const` local tested by `typeof`: the right operand reads a String.
#[test]
fn t1_typeof_const_narrows() {
    proven(
        &t(
            "const message = params[0];",
            "if (typeof message === 'string' && message.startsWith('T')) return 1;",
        ),
        "startsWith",
    );
}

// KEEP t2: `||`: the right operand runs when the test failed.
#[test]
fn t2_or_does_not_narrow() {
    not_proven(
        &t(
            "const message = params[0];",
            "if (typeof message === 'string' || message.startsWith('T')) return 1;",
        ),
        "startsWith",
    );
}

// KEEP t3: `!==` true is the other side.
#[test]
fn t3_not_equal_does_not_narrow() {
    not_proven(
        &t(
            "const message = params[0];",
            "if (typeof message !== 'string' && message.startsWith('T')) return 1;",
        ),
        "startsWith",
    );
}

// KEEP t4: a `let` reassigned between the test and the use.
#[test]
fn t4_let_reassigned_between() {
    not_proven(
        &t(
            "let message = params[0];",
            "if (typeof message === 'string' && (message = params[1]) && message.startsWith('T')) return 1;",
        ),
        "startsWith",
    );
}

// KEEP t5: a closure that assigns the local runs between test and use.
#[test]
fn t5_closure_assigns() {
    not_proven(
        &t(
            "let message = params[0]; const g = () => { message = params[1]; return true; };",
            "if (typeof message === 'string' && g() && message.startsWith('T')) return 1;",
        ),
        "startsWith",
    );
}

/// `constructor` of an Array: `Array.prototype.constructor`, the intrinsic
/// %Array% (23.1.3.4), a data property no store reaches.
const C: &str = "
/*EXTRA*/
function k(a: any) { return a.constructor; }
console.log(k([1, String(Math.random()).length]), k([process.argv[2]]));
";

// c1: proven (NativeMember over Arrays).
#[test]
fn c1_array_constructor() {
    proven(&C.replace("/*EXTRA*/", ""), "constructor");
}

// KEEP c2: the program writes `constructor` onto Array.prototype.
#[test]
fn c2_array_constructor_written() {
    not_proven(
        &C.replace(
            "/*EXTRA*/",
            "(Array.prototype as any).constructor = process.argv[2];",
        ),
        "constructor",
    );
}

// KEEP c3: an own `constructor` stored on one of the Arrays.
#[test]
fn c3_own_constructor() {
    not_proven(
        &C.replace(
            "/*EXTRA*/",
            "const z: any = [1]; z.constructor = process.argv[2]; console.log(k(z));",
        ),
        "constructor",
    );
}

// KEEP m5: a stored `slice` on one class instance: the TypeError arm is gone.
#[test]
fn m5_instance_gains_slice() {
    not_proven(
        &m(
            "const v9: any = new V(9); v9.slice = () => 1; console.log(sl(v9));",
            "",
        ),
        "slice",
    );
}
