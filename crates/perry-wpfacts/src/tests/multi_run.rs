//! THROWSFIX: a site in a body the solve runs more than once (a for-in
//! body's run per key, FVA 4.3b; a summary instance per binding) is one
//! operation of the program: its verdict covers what the receiver holds in
//! EVERY run. Each fixture below has a for-in body whose runs see different
//! receivers: one run (or the shared body) holds a receiver the claim
//! names, another a receiver that refutes it (ECMA-262 14.7.5.6
//! ForIn/OfBodyEvaluation, 14.7.5.9 EnumerateObjectProperties, 10.1.8.1
//! OrdinaryGet). None may carry the claim.
use super::*;

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

fn one(src: &str) -> ProgramFacts {
    modules(&[("t", src)])
}

/// The verdicts (`Debug` form) and receivers of the sites of `key`.
fn verdicts(pf: &ProgramFacts, key: &str, call: bool) -> Vec<String> {
    let v: Vec<String> = pf
        .sites
        .iter()
        .filter(|s| s.key == key && s.is_call == call)
        .map(|s| format!("{:?} {:?}", s.verdict, s.receivers))
        .collect();
    eprintln!("MULTIRUN {key} call={call}: {v:?}");
    assert!(!v.is_empty(), "no site of {key}");
    v
}

/// No site of `key` carries a verdict named in `wrong`.
fn no_claim(pf: &ProgramFacts, key: &str, call: bool, wrong: &[&str]) {
    for v in verdicts(pf, key, call) {
        let name = v.split(|c: char| !c.is_alphanumeric()).next().unwrap_or("");
        assert!(!wrong.contains(&name), "wrong claim {name}: {v}");
    }
}

const O: &str = "const o = { a: 1, b: 2 };\n";

/// Throws: every run reads an object; none reads only undefined/null.
#[test]
fn throws_needs_every_run_nullish() {
    let pf = one(&format!(
        "{O}const o2: any = {{ a: {{ x: 1 }}, b: {{ x: 2 }} }};\n\
         for (const p in o) {{ const v = o2[p]; console.log(v.x); }}\n"
    ));
    no_claim(&pf, "x", false, &["Throws", "Dead"]);
}

/// Dead: the key itself holds a String in every run.
#[test]
fn dead_needs_every_run_empty_read() {
    let pf = one(&format!("{O}for (const p in o) {{ console.log(p.length); }}\n"));
    no_claim(&pf, "length", false, &["Dead", "Throws"]);
}

#[test]
fn dead_needs_every_run_empty_call() {
    let pf = one(&format!("{O}for (const p in o) {{ console.log(p.charAt(0)); }}\n"));
    no_claim(&pf, "charAt", true, &["Dead", "Throws"]);
}

/// The nullish two-kind arm: a String or undefined beside, in a run, an
/// object with its own `toUpperCase`.
#[test]
fn nullish_arm_needs_every_run() {
    let pf = one(&format!(
        "{O}const s: any = Math.random() < 0.5 ? \"q\" : undefined;\n\
         const o2: any = {{ a: {{ toUpperCase() {{ return 1; }} }}, b: 0 }};\n\
         for (const p in o) {{ const v = o2[p] || s; console.log(v.toUpperCase()); }}\n"
    ));
    no_claim(&pf, "toUpperCase", true, &["NativeMember", "Throws", "Dead"]);
}

/// NativeMember: a String beside, in a run, an object.
#[test]
fn native_member_needs_every_run() {
    let pf = one(&format!(
        "{O}const o2: any = {{ a: {{ toUpperCase() {{ return 1; }} }}, b: 0 }};\n\
         for (const p in o) {{ const v = o2[p] || \"q\"; console.log(v.toUpperCase()); }}\n"
    ));
    no_claim(&pf, "toUpperCase", true, &["NativeMember", "Throws", "Dead"]);
}

/// OwnLength: a String beside, in a run, an object with a `length` field.
#[test]
fn own_length_needs_every_run() {
    let pf = one(&format!(
        "{O}const o2: any = {{ a: {{ length: 5 }}, b: 0 }};\n\
         for (const p in o) {{ const v = o2[p] || \"q\"; console.log(v.length); }}\n"
    ));
    no_claim(&pf, "length", false, &["OwnLength", "Throws", "Dead"]);
}

/// T0: one layout beside, in a run, another layout with the key elsewhere.
#[test]
fn t0_read_needs_every_run() {
    let pf = one(&format!(
        "{O}const d = {{ x: 1 }};\n\
         const o2: any = {{ a: {{ y: 0, x: 2 }}, b: {{ y: 0, x: 3 }} }};\n\
         for (const p in o) {{ const v = o2[p] || d; console.log(v.x); }}\n"
    ));
    no_claim(&pf, "x", false, &["T0Mono", "Throws", "Dead"]);
}

/// T0 call: one body beside, in a run, another body.
#[test]
fn t0_call_needs_every_run() {
    let pf = one(&format!(
        "{O}const d = {{ m() {{ return 1; }} }};\n\
         const o2: any = {{ a: {{ m() {{ return 2; }} }}, b: 0 }};\n\
         for (const p in o) {{ const v = o2[p] || d; console.log(v.m()); }}\n"
    ));
    no_claim(&pf, "m", true, &["T0Mono", "Throws", "Dead"]);
}

/// OwnDataOrAbsent (array absent): an Array beside, in a run, an object
/// with an accessor under the key.
#[test]
fn own_data_or_absent_needs_every_run() {
    let pf = one(&format!(
        "{O}const d = [1, 2];\n\
         const o2: any = {{ a: {{ get foo() {{ return 1; }} }}, b: 0 }};\n\
         for (const p in o) {{ const v: any = o2[p] || d; console.log(v.foo); }}\n"
    ));
    no_claim(
        &pf,
        "foo",
        false,
        &["OwnDataOrAbsent", "T0Mono", "T0Set", "Throws", "Dead"],
    );
}

/// IntrinsicCall: a builtin function beside, in a run, a program function.
#[test]
fn intrinsic_call_needs_every_run() {
    let pf = one(&format!(
        "{O}const o2: any = {{ a: function () {{ return 7; }}, b: 0 }};\n\
         for (const p in o) {{ const f = o2[p] || Math.max; console.log(f(1, 2)); }}\n"
    ));
    let all: Vec<String> = pf
        .sites
        .iter()
        .map(|s| format!("{} {:?} {:?}", s.key, s.verdict, s.receivers))
        .collect();
    eprintln!("MULTIRUN intrinsic: {all:?}");
    assert!(
        !all.iter().any(|s| s.contains("IntrinsicCall")),
        "wrong IntrinsicCall: {all:?}"
    );
}

/// A class prototype's own method beside, in a run, an object's method.
#[test]
fn prototype_arm_needs_every_run() {
    let pf = one(&format!(
        "{O}class C {{ m() {{ return 1; }} }}\n\
         const o2: any = {{ a: {{ m() {{ return 2; }} }}, b: 0 }};\n\
         for (const p in o) {{ const v: any = o2[p] || C.prototype; console.log(v.m()); }}\n"
    ));
    no_claim(&pf, "m", true, &["T0Mono", "Throws", "Dead"]);
}

/// A builtin namespace's member beside, in a run, an object's method.
#[test]
fn namespace_arm_needs_every_run() {
    let pf = one(&format!(
        "{O}const o2: any = {{ a: {{ max() {{ return 0; }} }}, b: 0 }};\n\
         for (const p in o) {{ const v: any = o2[p] || Math; console.log(v.max(1, 2)); }}\n"
    ));
    no_claim(&pf, "max", true, &["NativeMember", "T0Mono", "Throws", "Dead"]);
}

/// NsExport: a module namespace beside, in a run, an ordinary object.
#[test]
fn ns_export_needs_every_run() {
    let main = "import * as ns from \"./a\";\n\
        const o = { a: 1, b: 2 };\n\
        const o2: any = { a: { v: 5 }, b: 0 };\n\
        for (const p in o) { const r: any = o2[p] || ns; console.log(r.v); }\n";
    let pf = modules(&[("a", "export let v = 1;\n"), ("main", main)]);
    no_claim(&pf, "v", false, &["NsExport", "Throws", "Dead"]);
}

/// Not a for-in run: a summarized function's per-binding instances (the
/// object it births is born per instance) still give the site a receiver.
#[test]
fn instance_birth_site_has_receivers() {
    let pf = one(
        "function f(k: any) { const q = { x: k }; return q.x; }\n\
         console.log(f(1), f(\"s\"));\n",
    );
    no_claim(&pf, "x", false, &["Throws", "Dead"]);
}
