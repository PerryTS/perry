// R9BIRTH7 (2026-11-16): reads of builtin objects whose answer the
// language fixes. `arguments.length` is an own data property of the
// arguments object (ECMA-262 10.4.4.6/10.4.4.7) that only a delete or a
// redefinition changes; a typed array's `length`, `byteLength` and
// `BYTES_PER_ELEMENT` are its builtin prototypes' (23.2.3, 23.2.7.2) unless
// the program defines an own one (10.4.5: an ordinary key on an
// integer-indexed exotic object) or writes a builtin prototype; a receiver
// that is an Array or a typed array reads each one's builtin `length`.
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

fn sites(pf: &crate::ProgramFacts, key: &str) -> Vec<String> {
    let mi = pf.module_paths.iter().position(|p| p == "a.ts").expect("a.ts") as u32;
    let v: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.module == mi && s.key == key && !s.access.is_store())
        .map(|s| format!("{:?}", s.verdict))
        .collect();
    for verdict in &v {
        eprintln!("RB7 .{key} verdict={verdict}");
    }
    v
}

fn all(pf: &crate::ProgramFacts, key: &str, want: &str) -> bool {
    let v = sites(pf, key);
    !v.is_empty() && v.iter().all(|x| x == want)
}

fn none(pf: &crate::ProgramFacts, key: &str, want: &str) -> bool {
    let v = sites(pf, key);
    !v.is_empty() && v.iter().all(|x| x != want)
}

// R1 (RED at rb6): `arguments.length` is own data.
#[test]
fn r9birth7_r1_arguments_length() {
    let pf = facts(
        "function f(a?: any, b?: any) { let n = 0; for (let i = 0; i < arguments.length; i++) n += arguments[i]; return n; }
console.log(f(1, Number(process.argv[2]) || 2));
",
    );
    assert!(all(&pf, "length", "OwnDataOrAbsent"), "R1");
}

// R2 (RED at rb6): a receiver that is only typed arrays.
#[test]
fn r9birth7_r2_typed_array_members() {
    let pf = facts(
        "const n = Number(process.argv[2]) || 3;
const ta = String(Math.random()) < '0.5' ? new Float32Array(n) : new Uint16Array(n);
console.log(ta.length, ta.byteLength, ta.BYTES_PER_ELEMENT);
",
    );
    assert!(all(&pf, "length", "NativeMember"), "R2 length");
    assert!(all(&pf, "byteLength", "NativeMember"), "R2 byteLength");
    assert!(all(&pf, "BYTES_PER_ELEMENT", "NativeMember"), "R2 BYTES_PER_ELEMENT");
}

// R3 (RED at rb6): an Array or a typed array: each one's builtin `length`.
#[test]
fn r9birth7_r3_array_or_typed_length() {
    let pf = facts(
        "function conv(a: any) { return String(Math.random()) < '0.5' ? a : new Float32Array(a); }
const x = conv([1, 2, Number(process.argv[2]) || 3]);
console.log(x.length);
",
    );
    assert!(all(&pf, "length", "NativeMember"), "R3");
}

// K1: `Array.prototype.push = f`: DEC 250 locks the image members of
// `Array.prototype` (the runtime refuses the store, `members::store_admitted`),
// so the read stays the intrinsic: kept as the existing rule's witness (the
// read form: a call `a.push(..)` lowers to the array intrinsic, no site).
#[test]
fn r9birth7_k1_array_proto_push_locked() {
    let pf = facts(
        "(Array.prototype as any).push = function (v: any) { return 0; };
const a: any[] = [Number(process.argv[2]) || 1];
const f = a.push;
console.log(typeof f, a.length);
",
    );
    assert!(all(&pf, "push", "NativeMember"), "K1");
}

// K2: an array with an own `push`.
#[test]
fn r9birth7_k2_own_push() {
    let pf = facts(
        "const a: any = [Number(process.argv[2]) || 1];
a.push = function (v: any) { return 7; };
const f = a.push;
console.log(typeof f);
",
    );
    assert!(none(&pf, "push", "NativeMember"), "K2");
}

// K3: an own `length` defined on a typed array (10.4.5: an ordinary key),
// alone and beside an Array.
#[test]
fn r9birth7_k3_typed_own_length() {
    let pf = facts(
        "const ta = new Float32Array(Number(process.argv[2]) || 3);
Object.defineProperty(ta, 'length', { value: 1 });
console.log(ta.length);
",
    );
    assert!(none(&pf, "length", "NativeMember"), "K3 alone");
    let pf = facts(
        "const ta = new Float32Array(Number(process.argv[2]) || 3);
Object.defineProperty(ta, 'length', { value: 1 });
const x: any = String(Math.random()) < '0.5' ? [1, 2] : ta;
console.log(x.length);
",
    );
    assert!(none(&pf, "length", "NativeMember"), "K3 mixed");
}

// K4: an Array or a plain object with `length`: no builtin member.
#[test]
fn r9birth7_k4_array_or_object_length() {
    let pf = facts(
        "const x: any = String(Math.random()) < '0.5' ? [1, 2] : { length: Number(process.argv[2]) || 3 };
console.log(x.length);
",
    );
    assert!(none(&pf, "length", "NativeMember"), "K4");
}

// K5: `arguments` handed to unknown code before the read.
#[test]
fn r9birth7_k5_arguments_to_unknown() {
    let pf = facts(
        "function f(a?: any) { const u: any = (globalThis as any)[String(Math.random())]; if (u) u(arguments); return arguments.length; }
console.log(f(Number(process.argv[2]) || 1));
",
    );
    assert!(none(&pf, "length", "OwnDataOrAbsent"), "K5");
}

// K6: `arguments.length` redefined as an accessor.
#[test]
fn r9birth7_k6_arguments_length_accessor() {
    let pf = facts(
        "function f(a?: any) { Object.defineProperty(arguments, 'length', { get() { return 9; } }); return arguments.length; }
console.log(f(Number(process.argv[2]) || 1));
",
    );
    assert!(none(&pf, "length", "OwnDataOrAbsent"), "K6");
}

// K7: `%TypedArray%.prototype.length` redefined.
#[test]
fn r9birth7_k7_typed_proto_redefined() {
    let pf = facts(
        "Object.defineProperty(Object.getPrototypeOf(Float32Array.prototype), 'length', { get() { return 7; } });
const ta = new Float32Array(Number(process.argv[2]) || 3);
console.log(ta.length);
",
    );
    assert!(none(&pf, "length", "NativeMember"), "K7");
}

// K8: a subclass of a typed array with its own `length` getter.
#[test]
fn r9birth7_k8_typed_subclass_getter() {
    let pf = facts(
        "class T extends Float32Array { get length() { return 5; } }
const ta = new T(Number(process.argv[2]) || 3);
console.log(ta.length);
",
    );
    assert!(none(&pf, "length", "NativeMember"), "K8");
}

fn proven(pf: &crate::ProgramFacts, key: &str) -> bool {
    let v = sites(pf, key);
    !v.is_empty() && v.iter().all(|x| !x.starts_with("No"))
}

fn unproven(pf: &crate::ProgramFacts, key: &str) -> bool {
    let v = sites(pf, key);
    !v.is_empty() && v.iter().all(|x| x.starts_with("No"))
}

// S1 (RED at rb6): a top-level `X.k = v` right after the class declaration
// (nothing runs between them) is the class object's own data static.
#[test]
fn r9birth7_s1_settled_static() {
    let pf = facts(
        "class T { v: any; constructor(v: any = (T as any).DEF) { this.v = v; } }
(T as any).DEF = 'd';
const t = new T(String(Math.random()) < '0.5' ? undefined : process.argv[2]);
console.log(t.v, (T as any).DEF);
",
    );
    assert!(proven(&pf, "DEF"), "S1");
}

// SK1: a construction between the declaration and the store reads the
// static before it exists.
#[test]
fn r9birth7_sk1_static_read_before_store() {
    let pf = facts(
        "class T { v: any; constructor() { this.v = (T as any).DEF; } }
const early = new T();
(T as any).DEF = 'd';
console.log(early.v, new T().v, process.argv[2]);
",
    );
    assert!(unproven(&pf, "DEF"), "SK1");
}

// SK2: a parent's static setter runs on the store: no own data static.
#[test]
fn r9birth7_sk2_parent_static_setter() {
    let pf = facts(
        "class P { static set DEF(v: any) { console.log('set', v); } }
class T extends P { v: any; constructor() { super(); this.v = (T as any).DEF; } }
(T as any).DEF = 'd';
console.log(new T().v, process.argv[2]);
",
    );
    assert!(unproven(&pf, "DEF"), "SK2");
}
