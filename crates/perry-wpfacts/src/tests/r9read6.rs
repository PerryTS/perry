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
    let mi = pf.module_paths.iter().position(|p| p == "a.ts").expect("a.ts") as u32;
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
    not_proven(&m("(Array.prototype as any).isV = process.argv[2];", ""), "isV");
}

// KEEP k2: Number.prototype gains `isV`: the Number arm reads it.
#[test]
fn k2_number_proto_written() {
    not_proven(&m("(Number.prototype as any).isV = process.argv[2];", "rd(n),"), "isV");
}

// KEEP k3: Object.prototype gains `clone`: the Array arm finds it.
#[test]
fn k3_object_proto_clone_written() {
    not_proven(&m("(Object.prototype as any).clone = function () { return 1; };", ""), "clone");
}

// KEEP k4: an unknown computed store onto W.prototype may add `isV`.
#[test]
fn k4_unknown_key_on_proto() {
    not_proven(&m("(W.prototype as any)[process.argv[2]] = 1;", ""), "isV");
}
