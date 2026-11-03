//! A published literal's own getter is a contract function of its owner
//! (`Gen::note_own_getters`, `OwnAccessor::Getter`): the reader runs it on
//! its stand-in of the literal and finds what its contract describes,
//! instead of a stub whose result is unknown. Module `a` owns the literal,
//! the last module reads it. Positive rows: whole proves the site and per
//! unit equals whole. Negative rows: both leave it unproven. A read through
//! another object whose prototype is the literal is weaker than whole,
//! never stronger.

use super::*;

fn modules(mods: &[(&str, &str)], per_unit: bool) -> ProgramFacts {
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
    // Re-export sources resolve as imports do.
    let mut cfg = FactsConfig::default();
    for (path, hir) in &hirs {
        for e in &hir.exports {
            let (perry_hir::Export::ReExport { source, .. }
            | perry_hir::Export::ExportAll { source }
            | perry_hir::Export::NamespaceReExport { source, .. }) = e
            else {
                continue;
            };
            if let Some(n) = source.strip_prefix("./") {
                cfg.export_sources
                    .entry(path.clone())
                    .or_default()
                    .insert(source.clone(), format!("{n}.ts"));
            }
        }
    }
    match per_unit {
        true => crate::compute_units(&list, &cfg),
        false => crate::compute(&list, &cfg),
    }
}

/// The verdicts of the reads (or, `call`, the calls) of `key` in the last
/// module, whole program and per unit.
fn verdicts(mods: &[(&str, &str)], key: &str, call: bool) -> (Vec<Verdict>, Vec<Verdict>) {
    let last = format!("{}.ts", mods[mods.len() - 1].0);
    let of = |pf: &ProgramFacts| {
        let mi = pf
            .module_paths
            .iter()
            .position(|p| *p == last)
            .unwrap_or_else(|| panic!("{last} in {:?}", pf.module_paths)) as u32;
        let mut s: Vec<_> = pf
            .sites
            .iter()
            .filter(|s| s.module == mi && s.key == key && s.is_call == call)
            .collect();
        s.sort_by_key(|s| s.byte_offset);
        let v: Vec<Verdict> = s.iter().map(|s| s.verdict.clone()).collect();
        assert!(!v.is_empty(), "no site .{key} in\n{}", pf.dump_text());
        v
    };
    (of(&modules(mods, false)), of(&modules(mods, true)))
}

fn proven(v: &[Verdict]) -> bool {
    v.iter().all(|v| !matches!(v, Verdict::No(_)))
}

fn unproven(v: &[Verdict]) -> bool {
    v.iter().all(|v| matches!(v, Verdict::No(_)))
}

/// Whole proves it and per unit equals whole.
fn positive(mods: &[(&str, &str)], key: &str, call: bool) {
    let (w, u) = verdicts(mods, key, call);
    assert!(proven(&w), "whole: {w:?}");
    assert_eq!(u, w, "per unit {u:?} != whole {w:?}");
}

/// Neither proves it.
fn negative(mods: &[(&str, &str)], key: &str, call: bool) {
    let (w, u) = verdicts(mods, key, call);
    assert!(unproven(&w), "whole: {w:?}");
    assert!(unproven(&u), "per unit: {u:?} (whole {w:?})");
}

const READ_ITEM: &str = r#"import { lib } from "./a";
let n = 0;
for (let i = 0; i < 100; i++) n += lib.item.kind;
console.log(n);
"#;

/// G2 (m1): the getter returns a fresh literal built from `this`.
const FRESH: &str = r#"export const lib = {
  base: 7,
  get item() { return { kind: this.base }; },
  plain() { return { kind: 80 }; },
};
"#;

const IDENT: &str = "createIdentifier(t: string) { return { kind: 80, text: t }; },";

/// The reader of the method rows: reads `item`, then calls the method.
const CALL_AFTER_ITEM: &str = r#"import { lib } from "./a";
let n = 0;
for (let i = 0; i < 100; i++) n += lib.item.kind + lib.createIdentifier("a").text.length;
console.log(n);
"#;

/// G1: the getter returns a field of `this`. Per unit the result is
/// unknown, as it is for a field method returning `this.inner` (the owner
/// runs a contract function on ⊤ beside the literal): never stronger than
/// whole.
#[test]
fn g1_a_getter_returning_a_field_literal_is_never_stronger_than_whole() {
    let a = r#"export const lib = {
  inner: { kind: 1 },
  get cur() { return this.inner; },
};
"#;
    let b = r#"import { lib } from "./a";
let n = 0;
for (let i = 0; i < 100; i++) n += lib.cur.kind;
console.log(n);
"#;
    let (w, u) = verdicts(&[("a", a), ("b", b)], "kind", false);
    assert!(proven(&w), "whole: {w:?}");
    assert!(unproven(&u) || u == w, "per unit {u:?}, whole {w:?}");
}

#[test]
fn g2_a_getter_returning_a_fresh_literal_is_proven_as_whole() {
    positive(&[("a", FRESH), ("b", READ_ITEM)], "kind", false);
}

#[test]
fn g9_a_getter_through_a_barrel_is_proven_as_whole() {
    let b = READ_ITEM.replace("./a", "./c");
    positive(
        &[
            ("a", FRESH),
            ("c", r#"export { lib } from "./a";"#),
            ("b", &b),
        ],
        "kind",
        false,
    );
}

/// G4: the getter hands `this` to unknown code, which rewrites the method.
/// Per unit this rests on the owner applying the escape of the getter's
/// `this` (lane MTE).
#[test]
fn g4_a_getter_escaping_this_leaves_the_method_unproven() {
    let a = format!(
        r#"(globalThis as any).zz = function (o: any) {{ o.createIdentifier = (t: string) => ({{ kind: 1, text: "xyz" }}); }};
export const lib = {{
  {IDENT}
  get item() {{ (globalThis as any).zz(this); return {{ kind: 312 }}; }},
}};
"#
    );
    negative(
        &[("a", &a), ("b", CALL_AFTER_ITEM)],
        "createIdentifier",
        true,
    );
}

/// G4b: the getter returns the literal itself; the reader rewrites the
/// method through it.
#[test]
fn g4b_a_getter_returning_this_leaves_the_method_unproven() {
    let a = format!("export const lib = {{\n  {IDENT}\n  get self() {{ return this; }},\n}};\n");
    let b = r#"import { lib } from "./a";
lib.self.createIdentifier = (globalThis as any).g;
let n = 0;
for (let i = 0; i < 100; i++) n += lib.createIdentifier("a").text.length;
console.log(n);
"#;
    negative(&[("a", &a), ("b", b)], "createIdentifier", true);
}

/// G5: the getter stores an unknown value into another unit's literal.
#[test]
fn g5_a_getter_storing_into_a_foreign_literal_leaves_it_unproven() {
    let c = format!("export const other = {{\n  {IDENT}\n}};\n");
    let a = r#"import { other } from "./c";
export const lib = {
  get item() { other.createIdentifier = (globalThis as any).g; return { kind: 3 }; },
};
"#;
    let b = r#"import { lib } from "./a";
import { other } from "./c";
let n = 0;
for (let i = 0; i < 100; i++) n += lib.item.kind + other.createIdentifier("a").text.length;
console.log(n);
"#;
    negative(&[("c", &c), ("a", a), ("b", b)], "createIdentifier", true);
}

/// G6: a setter stays a stub: the reader's store through it rewrites the
/// method.
#[test]
fn g6_a_setter_leaves_the_method_unproven() {
    let a = format!(
        "export const lib = {{\n  {IDENT}\n  set impl(f: any) {{ this.createIdentifier = f; }},\n}};\n"
    );
    let b = r#"import { lib } from "./a";
lib.impl = (globalThis as any).g;
let n = 0;
for (let i = 0; i < 100; i++) n += lib.createIdentifier("a").text.length;
console.log(n);
"#;
    negative(&[("a", &a), ("b", b)], "createIdentifier", true);
}

/// G8: an accessor under a computed key names no getter contract.
#[test]
fn g8_an_accessor_under_a_computed_key_leaves_the_method_unproven() {
    let a = format!(
        r#"export const lib = {{
  {IDENT}
}};
Object.defineProperty(lib, (globalThis as any).k, {{ get() {{ return (globalThis as any).g; }} }});
"#
    );
    let b = r#"import { lib } from "./a";
let n = 0;
for (let i = 0; i < 100; i++) n += lib[(globalThis as any).k]("a").text.length + lib.createIdentifier("a").text.length;
console.log(n);
"#;
    negative(&[("a", &a), ("b", b)], "createIdentifier", true);
}

/// G7: read through an object whose prototype is the literal: the
/// contract admits no other receiver, so per unit stays unproven, weaker
/// than whole and never stronger.
#[test]
fn g7_a_getter_on_a_foreign_receiver_is_weaker_than_whole() {
    let b = r#"import { lib } from "./a";
const o = Object.create(lib);
let n = 0;
for (let i = 0; i < 100; i++) n += o.item.kind;
console.log(n);
"#;
    let (w, u) = verdicts(&[("a", FRESH), ("b", b)], "kind", false);
    assert!(unproven(&u), "per unit: {u:?} (whole {w:?})");
}
