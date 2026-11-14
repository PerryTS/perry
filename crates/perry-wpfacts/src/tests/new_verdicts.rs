//! NEWVERD (DESIGN_SCOPE64 choice (b), owner 2026-11-13): four verdicts
//! that prove a site without a layout slot or a callee body.
//! - Dead: no value reaches the receiver (callee) at the fixpoint of the
//!   inclusion solve, and unknown code cannot enter the body with other
//!   values (an escaped function's parameters and receiver hold ⊤).
//! - IntrinsicCall: a value call whose callee set is exactly one builtin
//!   function object (its [[Call]], ECMA-262 10.3.1).
//! - NsExport: a named read on module namespace exotic objects only, of a
//!   name the module exports itself: the export binding's current value
//!   (10.4.6.8 [[Get]]); the claim is the binding, not a value.
//! - OwnDataOrAbsent: an own data lookup of the receiver or `undefined`:
//!   no accessor, no [[Prototype]] write, the chain proven not to hold the
//!   key (10.1.8.1 OrdinaryGet).
//! Each has a control that must stay unproven. Verdicts are compared by
//! their `Debug` name so the module builds before the verdicts exist.
use super::*;

fn one(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

fn modules(mods: &[(&str, &str)]) -> ProgramFacts {
    let hirs: Vec<(String, perry_hir::Module)> = mods
        .iter()
        .map(|(name, src)| {
            let path = format!("{name}.ts");
            let module = perry_parser::parse_typescript(src, &path).expect("parses");
            let mut hir = perry_hir::lower_module(&module, name, &path).expect("lowers");
            for imp in &mut hir.imports {
                if let Some(n) = imp.source.strip_prefix("./") {
                    imp.resolved_path = Some(format!("{n}.ts"));
                }
            }
            (path, hir)
        })
        .collect();
    let list: Vec<(String, &perry_hir::Module)> =
        hirs.iter().map(|(p, h)| (p.clone(), h)).collect();
    crate::compute(&list, &FactsConfig::default())
}

/// The verdict names of the sites of key `key` (`call`: calls, else
/// reads), in source order.
fn names(pf: &ProgramFacts, key: &str, call: bool) -> Vec<String> {
    let mut s: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == key && s.is_call == call)
        .collect();
    s.sort_by_key(|s| (s.module, s.byte_offset));
    let v: Vec<String> = s.iter().map(|s| format!("{:?}", s.verdict)).collect();
    eprintln!("NEWVERD {key} call={call}: {v:?}");
    v
}

fn is(v: &str, want: &str) -> bool {
    v == want || v.starts_with(&format!("{want}("))
}

fn all_pf(pf: &ProgramFacts, src: &str, key: &str, call: bool, want: &str) {
    let v = names(pf, key, call);
    assert!(
        !v.is_empty() && v.iter().all(|v| is(v, want)),
        "{key} call={call}: want {want}, got {v:?}\n{src}\n{}",
        pf.dump_text()
    );
}

fn none_pf(pf: &ProgramFacts, src: &str, key: &str, call: bool, not: &str) {
    let v = names(pf, key, call);
    assert!(
        !v.is_empty() && v.iter().all(|v| !is(v, not)),
        "{key} call={call}: want not {not}, got {v:?}\n{src}\n{}",
        pf.dump_text()
    );
}

fn all(src: &str, key: &str, call: bool, want: &str) {
    all_pf(&one(src), src, key, call, want)
}

fn none(src: &str, key: &str, call: bool, not: &str) {
    none_pf(&one(src), src, key, call, not)
}

// Dead (design fixture A). `C.enabled` is only ever `false`: after
// `if (this.enabled === false) return;` no receiver flows to `.files`.
const DEAD: &str = "const C: any = { enabled: false, files: {} as any, \
    get(k: string) { if (this.enabled === false) return; return this.files[k]; } };\n";

#[test]
fn dead_receiver_never_flows() {
    let src = format!("{DEAD}console.log(C.get(\"a\"));");
    all(&src, "files", false, "Dead");
}

#[test]
fn dead_control_enabled_written() {
    let src = format!("{DEAD}if (process.argv[2]) C.enabled = true;\nconsole.log(C.get(\"a\"));");
    none(&src, "files", false, "Dead");
}

#[test]
fn dead_control_method_given_to_unknown_code() {
    let src = format!(
        "declare function sink(x: any): void;\n{DEAD}sink(C.get);\nconsole.log(C.get(\"a\"));"
    );
    none(&src, "files", false, "Dead");
}

// OwnDataOrAbsent: a key stored after birth is not a birth field, but the
// read is an own data lookup or `undefined`.
#[test]
fn own_or_absent_named_store_after_birth() {
    let src = "const a: any = {};\nif (process.argv[2]) a.FOO = \"x\";\nconsole.log(a.FOO);";
    all(src, "FOO", false, "OwnDataOrAbsent");
}

#[test]
fn own_or_absent_control_object_prototype_written() {
    let src = "(Object.prototype as any).FOO = 1;\nconst a: any = {};\n\
        if (process.argv[2]) a.FOO = \"x\";\nconsole.log(a.FOO);";
    none(src, "FOO", false, "OwnDataOrAbsent");
}

#[test]
fn own_or_absent_control_given_to_unknown_code() {
    let src = "declare function sink(x: any): void;\nconst a: any = {};\n\
        if (process.argv[2]) a.FOO = \"x\";\nsink(a);\nconsole.log(a.FOO);";
    none(src, "FOO", false, "OwnDataOrAbsent");
}

#[test]
fn own_or_absent_control_getter_defined() {
    let src = "const a: any = {};\nif (process.argv[2]) a.FOO = \"x\";\n\
        Object.defineProperty(a, \"FOO\", { get() { return 1; } });\nconsole.log(a.FOO);";
    none(src, "FOO", false, "OwnDataOrAbsent");
}

/// The brief's input-key store: `k` may be `"__proto__"`, and storing an
/// object under it runs %Object.prototype%'s `__proto__` setter (B.2.2.1.2),
/// which makes the object the receiver's [[Prototype]]: the chain is the
/// stored value's, not proven absent.
#[test]
fn own_or_absent_control_input_key_object_value() {
    let src = "const a: any = {};\nconst k = process.argv[2];\na[k] = { n: 1 };\nconsole.log(a.FOO);";
    none(src, "FOO", false, "OwnDataOrAbsent");
}

/// `modules` in per-unit mode (`compute_units`, design §18).
fn units(mods: &[(&str, &str)]) -> ProgramFacts {
    let hirs: Vec<(String, perry_hir::Module)> = mods
        .iter()
        .map(|(name, src)| {
            let path = format!("{name}.ts");
            let module = perry_parser::parse_typescript(src, &path).expect("parses");
            let mut hir = perry_hir::lower_module(&module, name, &path).expect("lowers");
            for imp in &mut hir.imports {
                if let Some(n) = imp.source.strip_prefix("./") {
                    imp.resolved_path = Some(format!("{n}.ts"));
                }
            }
            (path, hir)
        })
        .collect();
    let list: Vec<(String, &perry_hir::Module)> =
        hirs.iter().map(|(p, h)| (p.clone(), h)).collect();
    crate::compute_units(&list, &FactsConfig::default())
}

// Dead across units: the parameter of `f` is a closed marker in the unit of `a`; the
// value `m` passes comes through the caller contract and sits beside it, so
// neither the read `o.files` nor the call `o.get()` is Dead, in either mode.
const XA: &str = "export function f(o: any) { return o.get() + o.files; }\n";
const XM: &str =
    "import { f } from \"./a\";\nconsole.log(f({ files: 1, get() { return 2; } }));\n";

#[test]
fn dead_control_value_from_another_unit() {
    let mods = [("a", XA), ("m", XM)];
    let src = format!("{XA}{XM}");
    for pf in [modules(&mods), units(&mods)] {
        none_pf(&pf, &src, "files", false, "Dead");
        none_pf(&pf, &src, "get", true, "Dead");
    }
}

// The whole-program Dead fixture is Dead; per unit the receiver comes from
// unmodeled code (TOP), so per unit claims no Dead (less than whole-program,
// which is allowed).
#[test]
fn dead_receiver_is_dead_in_whole_mode_and_not_claimed_per_unit() {
    let src = format!("{DEAD}console.log(C.get(\"a\"));");
    let whole = modules(&[("t", &src)]);
    all_pf(&whole, &src, "files", false, "Dead");
    let unit = units(&[("t", &src)]);
    let v = names(&unit, "files", false);
    assert!(!v.is_empty(), "no site for files");
    for x in &v {
        assert!(!is(x, "Dead"), "per unit must not claim Dead: {v:?}");
        assert!(is(x, "No"), "per unit verdict must be a No(..): {v:?}");
    }
}

// IntrinsicCall: a value call whose callee is one intrinsic function object,
// while no unknown code exists.
#[test]
fn intrinsic_call_one_builtin_callee() {
    let src = "const f = Math.sin;\nconst y = f(1);\nconst g = Math.cos;\nconst z = g(2);";
    all(src, "()", true, "IntrinsicCall");
}

/// The program stores another function onto the member: the callee set
/// holds it beside the intrinsic.
#[test]
fn intrinsic_call_control_member_replaced() {
    let src = "if (process.argv[2]) (Math as any).sin = (x: number) => x + 1;\n\
        const f = Math.sin;\nconst y = f(1);";
    none(src, "()", true, "IntrinsicCall");
}

/// A computed store whose key may be `sin` reaches the member too.
#[test]
fn intrinsic_call_control_computed_store() {
    let src = "const k = process.argv[2];\n(Math as any)[k] = (x: number) => x + 1;\n\
        const f = Math.sin;\nconst y = f(1);";
    none(src, "()", true, "IntrinsicCall");
}

/// Unknown code exists: it holds the realm and may replace `Math.sin`; the
/// solve does not replay that write, so no claim.
#[test]
fn intrinsic_call_control_realm_held() {
    let src = "declare function sink(x: any): void;\nsink(0);\n\
        const f = Math.sin;\nconst y = f(1);";
    none(src, "()", true, "IntrinsicCall");
    let pf = one(src);
    assert!(
        pf.sites.iter().any(|s| s.key == "()"
            && matches!(&s.verdict, crate::Verdict::No(r) if r.starts_with("intrinsic callee: unknown code holds the realm"))),
        "{}",
        pf.dump_text()
    );
}

// NsExport: a named read on the program's own module namespaces of a name
// the module exports itself.
const NS_A: &str = "export let v = 1;\nexport const w = { n: 2 };\nexport function bump() { v++; }";

#[test]
fn ns_export_own_binding() {
    let main = "import * as ns from \"./a\";\nconst y = ns.v;\nconst z = ns.w;";
    let pf = modules(&[("a", NS_A), ("main", main)]);
    all_pf(&pf, main, "v", false, "NsExport");
    all_pf(&pf, main, "w", false, "NsExport");
}

/// The claim is the binding: `v` reassigned by its module is still read
/// through the binding (10.4.6.8), and unknown code holding the namespace
/// cannot write it (10.4.6.9).
#[test]
fn ns_export_reassigned_and_given_to_unknown_code() {
    let main = "declare function sink(x: any): void;\nimport * as ns from \"./a\";\n\
        sink(ns);\nns.bump();\nconst y = ns.v;";
    let pf = modules(&[("a", NS_A), ("main", main)]);
    all_pf(&pf, main, "v", false, "NsExport");
}

/// The receiver may be an ordinary object: no claim.
#[test]
fn ns_export_control_receiver_may_be_object() {
    let main = "import * as ns from \"./a\";\n\
        const o: any = process.argv[2] ? ns : { v: 3 };\nconst y = o.v;";
    let pf = modules(&[("a", NS_A), ("main", main)]);
    none_pf(&pf, main, "v", false, "NsExport");
}

/// A name the module does not export itself (`export *` from another): no
/// claim (its resolution is not modeled here).
#[test]
fn ns_export_control_star_reexport() {
    let main = "import * as ns from \"./a\";\nconst y = ns.u;";
    let pf = modules(&[("b", "export const u = 4;"), ("a", "export * from \"./b\";"), ("main", main)]);
    none_pf(&pf, main, "u", false, "NsExport");
}
