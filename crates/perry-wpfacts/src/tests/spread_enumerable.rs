//! SPREAD-ENUMERABLE: `Object.assign(t, s)` and `{...s}` copy the source's
//! own ENUMERABLE keys only (ECMA-262 20.1.2.1 step 3.a.iii, 7.3.25
//! CopyDataProperties), by the one predicate a for-in uses (ENUM3
//! `Node::enum_key`, an index name, or an escaped source). Not copied: a
//! class prototype's methods and `constructor` (15.7.14, 10.2.8
//! DefineMethodProperty enumerable false), a function's `prototype` /
//! `name` / `length`, an array's `length`. A key the spread only READS on
//! the source does not become enumerable for a later for-in over it.
use super::*;

fn v(src: &str) -> crate::derive::KindFact {
    let pf = facts_with(src, interp_cfg());
    field(tmpl(&pf, "R"), "v").kind.clone()
}

const R: &str = "class R { v: any; constructor(v: any) { this.v = v; } }\nclass K { x = 1; }\nclass Qz { m() { return 1; } }\n";

fn no_qz(k: &crate::derive::KindFact) -> bool {
    !k.top && k.funcs == 0 && !k.objs.iter().any(|o| o.contains("Qz"))
}

fn just_k(k: &crate::derive::KindFact) -> bool {
    !k.top && k.objs.len() == 1 && k.objs[0].ends_with(":K")
}

/// (a) a class prototype's method and `constructor` are own but not
/// enumerable: neither form copies them.
#[test]
fn a_class_prototype_members_not_copied() {
    for copy in [
        "const t: any = {}; Object.assign(t, Qz.prototype);",
        "const t: any = { ...Qz.prototype };",
    ] {
        for key in ["m", "constructor"] {
            let k = v(&format!("{R}{copy}\nconst r = new R(t.{key});\nconsole.log(r.v);\n"));
            eprintln!("a {copy} {key} {k:?}");
            assert!(no_qz(&k), "{copy} t.{key}: {k:?}");
        }
    }
}

/// (b) an array's `length`, a function's `prototype`: not enumerable.
/// (An empty array: its elements are any-key values, which a copy gives
/// every key of the target.)
#[test]
fn b_array_length_and_function_prototype_not_copied() {
    let k = v(&format!("{R}const t: any = {{}}; Object.assign(t, []);\nconst r = new R(t.length);\nconsole.log(r.v, t[0]);\n"));
    let c = v(&format!("{R}const t: any = {{}};\nconst r = new R(t.length);\nconsole.log(r.v, t[0]);\n"));
    eprintln!("b length {k:?} control {c:?}");
    assert!(!k.top && k.prims == c.prims && k.objs.is_empty(), "t.length {k:?} vs {c:?}");
    let k = v(&format!("{R}function fq(this: any) {{ this.x = 1; }}\nconst t: any = {{}}; Object.assign(t, fq);\nconst r = new R(t.prototype);\nconsole.log(r.v, new (fq as any)());\n"));
    eprintln!("b prototype {k:?}");
    assert!(!k.top && k.objs.is_empty() && k.funcs == 0, "t.prototype {k:?}");
}

/// (c) keys made by assignment or by a literal are enumerable: copied.
#[test]
fn c_assigned_and_literal_keys_copied() {
    for copy in [
        "const s: any = {}; s.k = new K(); const t: any = {}; Object.assign(t, s);",
        "const s: any = { k: new K() }; const t: any = {}; Object.assign(t, s);",
        "const s: any = {}; s.k = new K(); const t: any = { ...s };",
        "const s: any = { k: new K() }; const t: any = { ...s };",
    ] {
        let k = v(&format!("{R}{copy}\nconst r = new R(t.k);\nconsole.log(r.v);\n"));
        eprintln!("c {copy} {k:?}");
        assert!(just_k(&k), "{copy}: {k:?}");
    }
}

/// (d) defineProperty with `enumerable: true`, or with a descriptor whose
/// flags are unknown: copied (conservative).
#[test]
fn d_defined_enumerable_or_unknown_copied() {
    for desc in [
        "{ value: new K(), enumerable: true }",
        "mk()",
    ] {
        let k = v(&format!("{R}function mk(): any {{ const d: any = {{}}; d.value = new K(); if (Math.random() > 2) d.enumerable = true; return d; }}\nconst s: any = {{}}; Object.defineProperty(s, \"h\", {desc}); const t: any = {{}}; Object.assign(t, s);\nconst r = new R(t.h);\nconsole.log(r.v);\n"));
        eprintln!("d {desc} {k:?}");
        assert!(just_k(&k), "{desc}: {k:?}");
    }
}

/// (d) defineProperty without `enumerable: true` makes a non-enumerable
/// property: not copied.
#[test]
#[ignore = "OVMB49 open: a data defineProperty stores through `field` (enumerable); exact needs the descriptor's enumerable flag"]
fn d_defined_non_enumerable_not_copied() {
    let k = v(&format!("{R}const s: any = {{}}; Object.defineProperty(s, \"h\", {{ value: new K() }}); const t: any = {{}}; Object.assign(t, s);\nconst r = new R(t.h);\nconsole.log(r.v);\n"));
    eprintln!("d non-enum {k:?}");
    assert!(!k.top && k.objs.is_empty(), "t.h {k:?}");
}

/// (e) the spread READS each source key; that read does not make a
/// non-enumerable method an enumerable key for a later for-in.
#[test]
fn e_spread_read_does_not_make_key_enumerable() {
    let k = v(&format!("{R}const t: any = {{}}; Object.assign(t, Qz.prototype);\nlet out: any = undefined;\nconst p0: any = Qz.prototype;\nfor (const p in p0) {{ out = p0[p]; }}\nconst r = new R(out);\nconsole.log(r.v, t);\n"));
    eprintln!("e {k:?}");
    assert!(no_qz(&k), "for-in after spread {k:?}");
}

/// (f) control: an unknown source gives the target unknown keys, as before.
#[test]
fn f_unknown_source_copies_unknown() {
    let k = v(&format!("{R}const g: any = globalThis as any; const s: any = g[String(Math.random())]();\nconst t: any = {{}}; Object.assign(t, s);\nconst r = new R(t.q);\nconsole.log(r.v);\n"));
    eprintln!("f {k:?}");
    assert!(k.top, "t.q {k:?}");
}
