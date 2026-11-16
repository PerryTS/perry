// R9READ5 (2026-11-16): `new Y( args )` of a plain class in the settled
// window of the top-level static stores (`gen::settled`).
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

/// Verdicts of the read / call sites `<recv>.<key>`.
fn reads(pf: &crate::ProgramFacts, key: &str) -> Vec<String> {
    let mi = pf.module_paths.iter().position(|p| p == "a.ts").expect("a.ts") as u32;
    let v: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.module == mi && s.key == key && !s.access.is_store())
        .map(|s| format!("{:?}", s.verdict))
        .collect();
    for verdict in &v {
        eprintln!("RR5 .{key} verdict={verdict}");
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

const S: &str = "/*PRE*/
class V {
  x: number; y: number; z: number;
  constructor(x = 0, y = 0, z = 0) {
    V.prototype.isV = true;
    this.x = x;
    this.y = y;
    this.z = z;
  }
  clone() { return new V(this.x, this.y, this.z); }
  /*VBODY*/
}
class O {
  up: V; m: boolean;
  constructor() {
    this.up = O.DEFAULT_UP.clone();
    this.m = O.DEFAULT_M;
  }
}
/*MID*/
O.DEFAULT_UP = new V(0, 1, 0);
O.DEFAULT_M = true;
/*EXTRA*/
const o = new O();
console.log(o.up.y, o.m, String(Math.random()).length);
";

// b1: `new V( 0, 1, 0 )` runs only V's constructor, which stores its
// parameters and a constant into `this` / `V.prototype` (10.2.2, 10.1.13):
// no code of the program runs, so the window stays open past it and both
// statics are the class object's own data properties from then on.
#[test]
fn r9read5_b1_inert_construction_keeps_window() {
    proven(S, "DEFAULT_UP");
    proven(S, "DEFAULT_M");
}

// KEEP b2: V has a parent class: its constructor runs the parent's.
#[test]
fn r9read5_b2_parent_class_kept() {
    let src = S
        .replace("/*PRE*/", "class B { constructor() { console.log(String(Math.random())); } }")
        .replace("class V {", "class V extends B {")
        .replace("constructor(x = 0, y = 0, z = 0) {", "constructor(x = 0, y = 0, z = 0) {\n    super();");
    not_proven(&src, "DEFAULT_M");
}

// KEEP b3: the constructor calls a method.
#[test]
fn r9read5_b3_ctor_call_kept() {
    let src = S
        .replace("this.z = z;", "this.z = z;\n    this.log();")
        .replace("/*VBODY*/", "log() { console.log(String(Math.random())); }");
    not_proven(&src, "DEFAULT_M");
}

// KEEP b4: a setter for a stored key on V.prototype (10.1.9.2 runs it).
#[test]
fn r9read5_b4_class_setter_kept() {
    let src = S
        .replace("x: number; y: number; z: number;", "x: number; z: number; _y = 0;")
        .replace("/*VBODY*/", "get y() { return this._y; } set y(v: number) { console.log(String(Math.random())); this._y = v; }");
    not_proven(&src, "DEFAULT_M");
}

// KEEP b5: an accessor for a stored key defined anywhere in the program.
#[test]
fn r9read5_b5_defined_setter_kept() {
    let src = S.replace(
        "/*EXTRA*/",
        "if (process.argv[2]) Object.defineProperty(Object.prototype, \"y\", { set(v: any) { console.log(v); }, configurable: true });",
    );
    not_proven(&src, "DEFAULT_M");
}

// KEEP b6: a parameter default that calls code.
#[test]
fn r9read5_b6_default_call_kept() {
    let src = S.replace("z = 0) {", "z = Number(String(Math.random()))) {");
    not_proven(&src, "DEFAULT_M");
}

// KEEP b7: the constructed class is declared after the store (TDZ: the
// construction throws).
#[test]
fn r9read5_b7_tdz_class_kept() {
    let src = S
        .replace("O.DEFAULT_UP = new V(0, 1, 0);", "O.DEFAULT_UP = new W(0, 1, 0);")
        .replace("/*EXTRA*/", "class W { a: number; constructor(a = 0, b = 0, c = 0) { this.a = a; } clone() { return this; } }");
    not_proven(&src, "DEFAULT_M");
}

// b8: an accessor under a stored key on an unrelated class's prototype is
// not on a fresh V's chain (V.prototype, Object.prototype): the window
// stays open.
#[test]
fn r9read5_b8_unrelated_accessor_keeps_window() {
    let src = S.replace(
        "/*PRE*/",
        "class Q { _x = 0; get x() { return this._x; } set x(v: number) { console.log(v); this._x = v; } }\nconst q = new Q(); q.x = Number(process.argv[2] || \"1\"); console.log(q.x);",
    );
    let src = src.replace("/*PRE*/", "");
    proven(&src, "DEFAULT_M");
}

// KEEP b9: a setter for a stored key defined on V.prototype itself.
#[test]
fn r9read5_b9_proto_defined_setter_kept() {
    let src = S.replace(
        "/*EXTRA*/",
        "if (process.argv[2]) Object.defineProperty(V.prototype, \"y\", { set(v: any) { console.log(v); }, configurable: true });",
    );
    not_proven(&src, "DEFAULT_M");
}
