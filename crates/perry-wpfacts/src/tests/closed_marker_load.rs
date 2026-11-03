//! CHB3: a load from a closed marker adds no absence `undefined`. A closed
//! marker (a closed function's parameter, a published method's receiver
//! with published receivers, a described parameter) is use tracking: every
//! value its parameter can hold is beside it, and each of those reads its
//! own absence. Presence is "present on every value behind the marker", not
//! "always present": a value beside it without the key still adds it.
//!
//! And a declared field (`x: T;`) defines `undefined` at field-init time:
//! that `undefined` is unobservable only when the constructor body stores
//! the key on every path first, not when the key is merely present.
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
    let cfg = FactsConfig::default();
    match per_unit {
        true => crate::compute_units(&list, &cfg),
        false => crate::compute(&list, &cfg),
    }
}

fn kind_of(pf: &ProgramFacts, class: &str, key: &str) -> String {
    let t = pf
        .templates
        .iter()
        .find(|t| t.display.ends_with(&format!(":{class}")))
        .unwrap_or_else(|| panic!("{class}\n{}", pf.dump_text()));
    let f = t.fields.iter().find(|f| f.key == key).expect("field");
    f.kind.desc()
}

/// Per unit is never stronger than whole.
fn never_stronger(mods: &[(&str, &str)]) {
    let pu = modules(mods, true);
    let pw = modules(mods, false);
    for s in &pu.sites {
        if matches!(s.verdict, Verdict::No(_)) {
            continue;
        }
        let w = pw
            .sites
            .iter()
            .find(|x| x.module == s.module && x.byte_offset == s.byte_offset && x.key == s.key);
        assert!(
            w.is_some_and(|w| !matches!(w.verdict, Verdict::No(_))),
            "per unit proves {} at {} but whole does not",
            s.key,
            s.byte_offset
        );
    }
}

const MAIN: &str = "import { E } from './a'; const e = new E(true); e.m(); console.log(e.c);";

fn class(ctor: &str, extra: &str) -> String {
    format!(
        "export class E {{ c: any = 0; constructor(b: boolean) {{ {ctor} }} \
         m(): void {{ this.c = (this as any).t; }} {extra} }}"
    )
}

#[test]
fn a_published_method_reads_a_key_every_instance_holds() {
    // `t` is a definite own key of every E: per unit the published `m`'s
    // receiver marker added `undefined` to the read anyway (base:
    // `num|undef|builtin*1`), whole mode did not.
    let a = class("", "t = new Map<string, number>();");
    let mods = [("a", a.as_str()), ("m", MAIN)];
    for per_unit in [true, false] {
        let k = kind_of(&modules(&mods, per_unit), "E", "c");
        assert!(
            k.contains("builtin") && !k.contains("undef"),
            "unit={per_unit} c: {k}"
        );
    }
    never_stronger(&mods);
}

#[test]
fn a_constructor_path_without_the_key_reads_undefined() {
    // One constructor path never stores `t`: the instance beside the marker
    // reads `undefined` for it, marker or not.
    let a = class("if (b) (this as any).t = new Map<string, number>();", "");
    let mods = [("a", a.as_str()), ("m", MAIN)];
    for per_unit in [true, false] {
        let k = kind_of(&modules(&mods, per_unit), "E", "c");
        assert!(
            k.contains("builtin") && k.contains("undef"),
            "unit={per_unit} c: {k}"
        );
    }
    never_stronger(&mods);
}

#[test]
fn a_deleted_key_reads_undefined() {
    let a = class(
        "",
        "t = new Map<string, number>(); drop(): void { delete (this as any).t; }",
    );
    let main = "import { E } from './a'; const e = new E(true); e.drop(); e.m(); console.log(e.c);";
    let mods = [("a", a.as_str()), ("m", main)];
    for per_unit in [true, false] {
        let k = kind_of(&modules(&mods, per_unit), "E", "c");
        // ⊤ (per unit: the delete reshapes what another unit may hold) holds it too.
        assert!(k.contains("undef") || k == "top", "unit={per_unit} c: {k}");
    }
    never_stronger(&mods);
}

fn declared(ctor: &str) -> String {
    format!(
        "class E {{ t: Map<string, number>; constructor(b: boolean) {{ {ctor} }} \
         at(k: string): number {{ return this.t.get(k) ?? 0; }} }} \
         const e = new E(Math.random() > 2); console.log(e.at('a'));"
    )
}

#[test]
fn a_declared_field_one_path_leaves_unset_holds_undefined() {
    // Base: kind `builtin*1` and `get` NativeMember, while `new E(false)`
    // keeps the declared `undefined` (the call throws a TypeError).
    let pf = facts(&declared("if (b) this.t = new Map<string, number>();"));
    let k = kind_of(&pf, "E", "t");
    assert!(k.contains("undef") && k.contains("builtin"), "t: {k}");
    let get: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == "get" && s.is_call)
        .collect();
    assert!(
        !get.is_empty() && get.iter().all(|s| s.verdict != Verdict::NativeMember),
        "{get:?}"
    );
}

#[test]
fn a_declared_field_every_path_stores_holds_no_undefined() {
    let pf = facts(&declared(
        "if (b) this.t = new Map<string, number>(); else this.t = new Map<string, number>();",
    ));
    let k = kind_of(&pf, "E", "t");
    assert!(!k.contains("undef") && k.contains("builtin"), "t: {k}");
    let get: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.key == "get" && s.is_call)
        .collect();
    assert!(
        !get.is_empty() && get.iter().all(|s| s.verdict == Verdict::NativeMember),
        "{get:?}"
    );
}
