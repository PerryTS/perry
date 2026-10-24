//! Per-unit mode: a namespace import of another unit's module (design
//! §18.2): the namespace object's keys are the module's export names, and
//! each key holds what a named import of it binds.

use super::*;

fn units_of(mods: &[(&str, &str)]) -> ProgramFacts {
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

fn verdicts(pf: &ProgramFacts, key: &str) -> Vec<String> {
    pf.sites
        .iter()
        .filter(|s| s.key == key)
        .map(|s| format!("{:?}", s.verdict))
        .collect()
}

const DEF: &str = "export const E = { A: 1, B: 2 };\nexport function f() { return 1; }";

#[test]
fn a_namespace_key_holds_what_a_named_import_binds() {
    // `p.E` is `import { E }`: the same contract, so `.A` holds on E's
    // layout through the namespace as through the named import.
    let named = units_of(&[
        ("a", DEF),
        (
            "m",
            "import { E } from './a'; function g() { return E.A; } console.log(g());",
        ),
    ]);
    assert_eq!(verdicts(&named, "A"), ["T0Mono"], "{}", named.dump_text());
    let ns = units_of(&[
        ("a", DEF),
        (
            "m",
            "import * as p from './a'; function g() { return p.E.A + p.f(); } console.log(g());",
        ),
    ]);
    assert_eq!(verdicts(&ns, "A"), ["T0Mono"], "{}", ns.dump_text());
    // A key that is no export reads `undefined` (the namespace has no
    // such key): nothing is proven through it.
    let pf = units_of(&[
        ("a", DEF),
        (
            "m",
            "import * as p from './a'; function g() { return (p as any).Z.A; } console.log(g());",
        ),
    ]);
    assert!(
        verdicts(&pf, "A").iter().all(|v| v.starts_with("No")),
        "{}",
        pf.dump_text()
    );
}

#[test]
fn a_namespace_with_an_export_star_from_outside_is_open() {
    // Soundness: `export *` from a module outside the program may add any
    // key: the namespace is open, every key may be unknown.
    let a = "export const E = { A: 1 };\nexport * from 'outside';";
    let pf = units_of(&[
        ("a", a),
        (
            "m",
            "import * as p from './a'; function g() { return (p as any).Q.A; } console.log(g());",
        ),
    ]);
    let s: Vec<_> = pf.sites.iter().filter(|s| s.key == "A").collect();
    assert!(
        !s.is_empty() && s.iter().all(|s| s.top),
        "{}",
        pf.dump_text()
    );
    // An `export *` inside the program adds the source's keys: `p.F` is a
    // key, holding what `import { F } from './a'` binds (⊤: a re-export
    // has no contract at `a`), not only `undefined`.
    let pf = units_of(&[
        ("b", "export const F = { A: 1 };"),
        ("a", "export * from './b';"),
        (
            "m",
            "import * as p from './a'; function g() { return p.F.A; } console.log(g());",
        ),
    ]);
    let s: Vec<_> = pf.sites.iter().filter(|s| s.key == "A").collect();
    assert!(
        !s.is_empty() && s.iter().all(|s| s.top),
        "{}",
        pf.dump_text()
    );
}

/// `units_of` with the compile driver's dynamic edges (collect_modules):
/// `import('./x')` folds into a static import of the same source as
/// `is_dynamic_target`, else it is a new edge with no specifiers and
/// `is_dynamic`.
fn units_dyn(mods: &[(&str, &str)]) -> ProgramFacts {
    let hirs: Vec<(String, perry_hir::Module)> = mods
        .iter()
        .map(|(name, src)| {
            let path = format!("{name}.ts");
            let module = perry_parser::parse_typescript(src, &path).expect("parses");
            let mut hir = perry_hir::lower_module(&module, name, &path).expect("lowers");
            let targets: Vec<String> = src
                .split("import('")
                .skip(1)
                .filter_map(|s| s.split('\'').next().map(str::to_string))
                .collect();
            for source in targets {
                if let Some(i) = hir
                    .imports
                    .iter_mut()
                    .find(|i| i.source == source && !i.is_dynamic)
                {
                    i.is_dynamic_target = true;
                    continue;
                }
                hir.imports.push(perry_hir::Import {
                    source,
                    specifiers: Vec::new(),
                    is_native: false,
                    module_kind: Default::default(),
                    resolved_path: None,
                    type_only: false,
                    runtime_erased: false,
                    is_dynamic: true,
                    is_dynamic_target: false,
                    is_deferred_require: false,
                    is_adopted_require: false,
                });
            }
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

#[test]
fn a_dynamically_imported_namespace_reaches_its_importer_undescribed() {
    // Soundness: `import()` hands the importer the namespace object itself
    // (⊤ there, no stand-in), so its stores are no contract use the census
    // reports: the exporter escapes its namespace, every export with it.
    // Also when the driver folds the edge into a static import of the same
    // module (`is_dynamic_target`).
    let a = "export const E = { A: 1 };\nexport const F = 1;\nfunction g() { return E.A; }\nconsole.log(g());";
    for m in [
        "async function h() { const p: any = await import('./a'); p.E.A = 'x'; }\nh();",
        "import { F } from './a';\nasync function h() { const p: any = await import('./a'); p.E.A = 'x'; }\nh(); console.log(F);",
    ] {
        let pf = units_dyn(&[("a", a), ("m", m)]);
        let e = pf
            .templates
            .iter()
            .find(|t| t.module == Some(0) && t.fields.iter().any(|f| f.key == "A"))
            .unwrap_or_else(|| panic!("E in\n{}", pf.dump_text()));
        let x = e.fields.iter().find(|f| f.key == "A").expect("field A");
        assert!(e.escape.is_some() && !x.readonly, "{m}\n{}", pf.dump_text());
    }
}

const ENUM: &str = "export enum C { X, Y }\nexport const enum K { P = 'p', Q = 'q' }";

#[test]
fn an_enum_object_is_a_contract_named_by_its_enum() {
    // §18.2 (b): the enum's object is named `<module>:enum <Name>` in every
    // unit, and published as an object contract: its reads hold on its
    // layout through a named import and through a namespace.
    for m in [
        "import { C, K } from './a'; function g() { return C.X + K.P; } console.log(g());",
        "import * as p from './a'; function g() { return p.C.X + p.K.P; } console.log(g());",
    ] {
        let pf = units_of(&[("a", ENUM), ("m", m)]);
        assert_eq!(verdicts(&pf, "X"), ["T0Mono"], "{}", pf.dump_text());
        assert_eq!(verdicts(&pf, "P"), ["T0Mono"], "{}", pf.dump_text());
        assert!(
            pf.templates.iter().any(|t| t.display == "a.ts:enum C"),
            "{}",
            pf.dump_text()
        );
    }
    // Soundness: an importer's store into a member is a store effect
    // (decision 17): the owner's member holds an unknown value.
    let m = "import { C } from './a'; (C as any).X = 'z'; function g() { return C.Y; } console.log(g());";
    let pf = units_of(&[("a", ENUM), ("m", m)]);
    assert!(pf.broken.is_empty(), "{:?}", pf.broken);
    let c = pf
        .templates
        .iter()
        .find(|t| t.display == "a.ts:enum C")
        .unwrap_or_else(|| panic!("enum C in\n{}", pf.dump_text()));
    let x = c.fields.iter().find(|f| f.key == "X").expect("member X");
    // Rewritten for DECISIONS 73 (principle: a store effect carries the kinds
    // it stores): the field holds the stored primitive, not ⊤.
    assert!(
        !x.readonly && x.kind.prims & crate::model::prim::STR != 0,
        "{}",
        pf.dump_text()
    );
    // A new key is the owner's shape transition (rewritten for design §16,
    // principle: a literal layout's new key is performed by its defining
    // unit): C gains `Z`. A computed key is beyond the contract: C breaks.
    let m =
        "import { C } from './a'; (C as any).Z = 1; function g() { return C.Y; } console.log(g());";
    let pf = units_of(&[("a", ENUM), ("m", m)]);
    assert!(pf.broken.is_empty(), "{:?}\n{}", pf.broken, pf.dump_text());
    assert!(super::unit_mode::gains_key(&pf, "Z"), "{}", pf.dump_text());
    let m = "import { C } from './a'; (C as any)[String(C.Y)] = 1; function g() { return C.Y; } console.log(g());";
    let pf = units_of(&[("a", ENUM), ("m", m)]);
    assert!(
        pf.broken.iter().any(|d| d == "a.ts:enum C"),
        "{:?}",
        pf.broken
    );
    assert!(
        verdicts(&pf, "Y").iter().all(|v| v.starts_with("No")),
        "{}",
        pf.dump_text()
    );
}

fn units_cfg(mods: &[(&str, &str)], cfg: &FactsConfig) -> ProgramFacts {
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
    crate::compute_units(&list, cfg)
}

const REEXP_A: &str =
    "export enum CommandTypes { A = \"a\", B = \"b\" }\nexport function f() { return 1; }";
const REEXP_M: &str = "import * as protocol from './a';\nexport const CommandNames: any = (protocol as any).CommandTypes;\nexport function g() { return 1; }\nconsole.log(g());";
const REEXP_U: &str = "import * as protocol from './a';\nimport { g } from './m';\nfunction h() { return protocol.CommandTypes.A; }\nconsole.log(h(), g());";

#[test]
fn interpreted_code_reaches_no_module_namespace() {
    // DECISIONS 65: interpreted code (decision 150) holds what escaped, the
    // globals and what is thrown, as in the whole-program solve; a module's
    // namespace object is not among them. `m` re-publishing the enum
    // through its own namespace does not break it for `u`.
    let mut cfg = FactsConfig::default();
    cfg.dynamic_code_reach.record_global("Function(...)".into());
    let pf = units_cfg(&[("a", REEXP_A), ("m", REEXP_M), ("u", REEXP_U)], &cfg);
    assert!(pf.broken.is_empty(), "{:?}", pf.broken);
    assert_eq!(verdicts(&pf, "A"), ["T0Mono"], "{}", pf.dump_text());
    // A direct eval can name a module's bindings: then the namespace object
    // reaches interpreted code, and what it holds is used beyond its contract.
    let mut cfg = FactsConfig::default();
    cfg.dynamic_code_reach.record_global("eval(...)".into());
    cfg.dynamic_code_reach.lexical_modules.insert("m.ts".into());
    let pf = units_cfg(&[("a", REEXP_A), ("m", REEXP_M), ("u", REEXP_U)], &cfg);
    assert!(
        pf.broken.iter().any(|d| d == "a.ts:enum CommandTypes"),
        "{:?}",
        pf.broken
    );
}

const NESTED_HEAD: &str = "export interface TargetRange { readonly range: number[] | number; readonly facts: number; thisNode?: number }\nfunction ces(range: TargetRange) { return range.facts; }\n";

#[test]
fn a_literal_nested_in_a_contract_result_is_a_nested_contract() {
    // Design §18.7 (DECISIONS 64): `get` is published (another unit may call
    // it), so its result literal leaves; the inner literals it holds under
    // `targetRange` leave with their own layout contracts instead of
    // escaping, and `ces` proves `range.facts` on them.
    for get in [
        "export function get(n: number) { if (n > 1) return { targetRange: { range: [n], facts: n, thisNode: undefined } }; return { targetRange: { range: n, facts: 0, thisNode: 1 } }; }",
        "export function get(n: number) { return { targetRange: { range: n, facts: 0, thisNode: 1 } }; }",
    ] {
        let src = format!("{NESTED_HEAD}{get}\nexport function actions(n: number) {{ const r = get(n); return ces(r.targetRange); }}\nconsole.log(actions(2));");
        let pf = units_of(&[("a", &src)]);
        let v = verdicts(&pf, "facts");
        assert!(!v.is_empty() && v.iter().all(|v| v.starts_with("T0")), "{v:?}\n{}", pf.dump_text());
    }
    // Another unit reads the nested literal through the contract.
    let a = "export function get(n: number) { return { targetRange: { range: n, facts: 0 } }; }";
    let u = "import { get } from './a'; function k() { return get(2).targetRange.facts; } console.log(k());";
    let pf = units_of(&[("a", a), ("u", u)]);
    assert_eq!(verdicts(&pf, "facts"), ["T0Mono"], "{}", pf.dump_text());
    // Soundness: another unit's store into the nested literal is a store
    // effect (decision 17); its new key is the owner's shape transition
    // (rewritten for design §16, principle: a literal layout's new key is
    // performed by its defining unit), a computed key breaks it (census).
    let u = "import { get } from './a'; const t = get(2).targetRange; (t as any).facts = 'z'; function k() { return t.range; } console.log(k());";
    let pf = units_of(&[("a", a), ("u", u)]);
    let inner = pf
        .templates
        .iter()
        .find(|t| t.fields.iter().any(|f| f.key == "facts"))
        .unwrap_or_else(|| panic!("{}", pf.dump_text()));
    let f = inner.fields.iter().find(|f| f.key == "facts").unwrap();
    // Rewritten for DECISIONS 73 (principle: a store effect carries the kinds
    // it stores): the field holds the stored primitive, not ⊤.
    assert!(
        !f.readonly && f.kind.prims & crate::model::prim::STR != 0,
        "{}",
        pf.dump_text()
    );
    let u = "import { get } from './a'; const t = get(2).targetRange; (t as any).extra = 1; function k() { return t.range; } console.log(k());";
    let pf = units_of(&[("a", a), ("u", u)]);
    assert!(pf.broken.is_empty(), "{:?}\n{}", pf.broken, pf.dump_text());
    assert!(
        super::unit_mode::gains_key(&pf, "extra"),
        "{}",
        pf.dump_text()
    );
    let u = "import { get } from './a'; const t = get(2).targetRange; (t as any)[String(t.facts)] = 1; function k() { return t.range; } console.log(k());";
    let pf = units_of(&[("a", a), ("u", u)]);
    assert!(
        pf.broken.iter().any(|d| d == &inner.display),
        "{} {:?}",
        inner.display,
        pf.broken
    );
    // A nested field holding anything but literal layouts leaves as before.
    let a = "export function get(n: number, o: any) { return { targetRange: n > 1 ? { range: n, facts: 0 } : o }; }";
    let u = "import { get } from './a'; function k() { return get(2, {}).targetRange.facts; } console.log(k());";
    let pf = units_of(&[("a", a), ("u", u)]);
    assert!(
        verdicts(&pf, "facts").iter().all(|v| v.starts_with("No")),
        "{}",
        pf.dump_text()
    );
}

#[test]
fn a_nested_contract_field_keeps_its_primitive_kinds_and_argument_literals_leave() {
    // Design §18.7: a nested field that may also hold a primitive tells the
    // reader so (its binding keeps the number kind beside the layout).
    let a = "export function get(n: number) { return { t: n > 1 ? { facts: n } : 5 }; }";
    let u = "import { get } from './a'; function k() { const t = get(2).t; return t.facts; } console.log(k());";
    let pf = units_of(&[("a", a), ("u", u)]);
    let inner = pf
        .templates
        .iter()
        .find(|t| t.fields.iter().any(|f| f.key == "facts"))
        .unwrap_or_else(|| panic!("{}", pf.dump_text()));
    let held: Vec<u16> = pf.bindings[1]
        .locals
        .iter()
        .filter(|(_, b)| inner.class_id.is_some_and(|c| b.class_ids.contains(&c)))
        .map(|(_, b)| b.prims)
        .collect();
    assert!(
        !held.is_empty() && held.iter().all(|&p| p & crate::model::prim::NUM != 0),
        "{held:?} {:?}",
        pf.bindings
    );
    // A literal another unit's closed function also gets as an argument is
    // described there with no nested fields: its fields leave as before, so
    // that callee's store under a nested literal is no unknown write.
    let a = "import { use } from './b';\nexport function get(n: number) { const inner = { deep: { facts: n }, other: { q: n } }; use(inner); return { t: inner }; }";
    let b = "export function use(x: any) { x.deep.facts = 'z'; }";
    let u =
        "import { get } from './a'; function k() { return get(2).t.deep.facts; } console.log(k());";
    let mods = [("a", a), ("b", b), ("u", u)];
    let (pf, wp) = (units_of(&mods), super::unit_mode::whole_of(&mods));
    let with = |pf: &ProgramFacts, k: &str| {
        pf.templates
            .iter()
            .find(|t| t.fields.iter().any(|f| f.key == k))
            .unwrap_or_else(|| panic!("{}", pf.dump_text()))
            .clone()
    };
    // DECISIONS 118: the reader runs `use`'s store `x.deep.facts = 'z'` on
    // its argument (a flow of `use`'s contract through the keyed reach
    // `deep`, no unknown write): `facts` is written with a string, and
    // `inner` gains no key, exactly as in the whole program.
    let (d, dw) = (with(&pf, "deep"), with(&wp, "deep"));
    assert!(
        d.fields.iter().all(|f| f.key == "deep" || f.key == "other"),
        "{}",
        pf.dump_text()
    );
    // Through `deep` only: `other` gains no key `facts`.
    let (o, ow) = (with(&pf, "q"), with(&wp, "q"));
    assert!(o.fields.iter().all(|f| f.key == "q"), "{}", pf.dump_text());
    assert_eq!(
        (o.fixed_layout, o.prefix_stable),
        (ow.fixed_layout, ow.prefix_stable)
    );
    assert_eq!(
        (d.fixed_layout, d.prefix_stable),
        (dw.fixed_layout, dw.prefix_stable)
    );
    let f = with(&pf, "facts")
        .fields
        .into_iter()
        .find(|f| f.key == "facts")
        .unwrap();
    let w = with(&wp, "facts")
        .fields
        .into_iter()
        .find(|f| f.key == "facts")
        .unwrap();
    assert!(!f.readonly && !w.readonly, "{}", pf.dump_text());
    assert!(
        w.kind.prims & crate::model::prim::STR != 0,
        "{}",
        wp.dump_text()
    );
    // Its number is `get`'s parameter, another unit's value per unit (⊤):
    // an open per-unit gap (reports/2026-10-22-CW2-perunit-parity.md).
    assert!(
        f.kind.top || f.kind.prims & crate::model::prim::STR != 0,
        "{}",
        pf.dump_text()
    );
}

#[test]
fn a_contract_result_keeps_its_primitive_kinds_and_a_closed_callee_argument_leaves() {
    // Design §18.8: the stand-in of a contract function returns the
    // primitive kinds its defining unit proved (`FnContract.ret_prim`).
    let a = "export function get(n: number) { return n > 1 ? { facts: n } : 5; }";
    let u = "import { get } from './a'; function k() { const r = get(2); return r.facts; } console.log(k());";
    let pf = units_of(&[("a", a), ("u", u)]);
    let lit = pf
        .templates
        .iter()
        .find(|t| t.fields.iter().any(|f| f.key == "facts"))
        .unwrap_or_else(|| panic!("{}", pf.dump_text()));
    let held: Vec<u16> = pf.bindings[1]
        .locals
        .iter()
        .filter(|(_, b)| lit.class_id.is_some_and(|c| b.class_ids.contains(&c)))
        .map(|(_, b)| b.prims)
        .collect();
    assert!(
        !held.is_empty() && held.iter().all(|&p| p & crate::model::prim::NUM != 0),
        "{held:?} {:?}",
        pf.bindings
    );
    // A literal a closed function of another unit (decision 13) also gets
    // as an argument is described there with no nested fields: that
    // callee's store under its nested literal comes back through its
    // parameter's reach uses, no write the owner misses.
    let a = "function use(x: any) { x.deep.facts = 'z'; }\nexport function get(n: number) { const inner = { deep: { facts: n } }; use(inner); return { t: inner }; }";
    let u =
        "import { get } from './a'; function k() { return get(2).t.deep.facts; } console.log(k());";
    let pf = units_of(&[("a", a), ("u", u)]);
    let deep = pf
        .templates
        .iter()
        .find(|t| t.fields.iter().any(|f| f.key == "facts"))
        .unwrap_or_else(|| panic!("{}", pf.dump_text()));
    let f = deep.fields.iter().find(|f| f.key == "facts").unwrap();
    assert!(!f.readonly && f.kind.top, "{}", pf.dump_text());
}

#[test]
fn an_exported_method_literal_is_born_by_its_defines() {
    // Design §18.9 (DECISIONS 67): an exported literal with a method is
    // lowered in source order, `(o => { define(o, "A", …); …; return o })
    // (new Shape(undefined, …))`, its method a module function of its own
    // unit. The defines are the literal's birth (one unit), the shape's
    // `undefined` placeholders are no field values, so `D` holds only the
    // method and leaves as a contract function: a unit calling `S.D(…)`
    // keeps `S` (no escape, no census break), and `.A` holds.
    let m = "export const S = { A: 'x', B: 'y', D(s: string) { return 'z' + s; } };\nexport function useA(x: boolean) { return x ? S.A : S.B; }";
    let callm = format!("{m}\nexport function k() {{ return S.D('q') + S.A; }}");
    for (a, u) in [
        (
            m.to_string(),
            "import { useA } from './a'; console.log(useA(true));",
        ),
        (callm, "import { k } from './a'; console.log(k());"),
        (
            m.to_string(),
            "import { S } from './a'; function g() { return S.A + S.D(S.B); } console.log(g());",
        ),
    ] {
        let pf = units_of(&[("a", &a), ("u", u)]);
        let v = verdicts(&pf, "A");
        assert!(
            !v.is_empty() && v.iter().all(|v| v.starts_with("T0")),
            "{v:?}\n{}",
            pf.dump_text()
        );
        assert!(pf.broken.is_empty(), "{:?}\n{}", pf.broken, pf.dump_text());
    }
    // Soundness: a unit that writes another value into `A` is a store
    // effect on the layout (decision 17), never a proof of the string.
    let u =
        "import { S } from './a'; (S as any).A = 1; function g() { return S.A; } console.log(g());";
    let pf = units_of(&[("a", m), ("u", u)]);
    let t = pf
        .templates
        .iter()
        .find(|t| t.fields.iter().any(|f| f.key == "D"))
        .unwrap_or_else(|| panic!("{}", pf.dump_text()));
    let f = t.fields.iter().find(|f| f.key == "A").unwrap();
    assert!(!f.readonly, "{}", pf.dump_text());
}

#[test]
fn a_rest_parameter_takes_only_the_extra_arguments() {
    // tssrc moduleNameResolver.ts `traceIfEnabled(state, diagnostic,
    // ...args)` (design §18.11): the callee's fixed parameters keep their
    // own uses, and only the arguments past them land in its rest array,
    // which escapes. A literal passed as a fixed argument stays described.
    const A: &str = "export function trace(s: any, d: string, ...args: any[]) { if (s.on) console.log(d, args.length); }";
    let fixed = units_of(&[
        ("a", A),
        (
            "m",
            "import { trace } from './a'; function mk() { return { on: true, n: 1 }; } function g() { const s = mk(); trace(s, 'x', 'y'); return s.n; } console.log(g());",
        ),
    ]);
    assert_eq!(verdicts(&fixed, "n"), ["T0Mono"], "{}", fixed.dump_text());
    // Soundness: the same literal as an extra argument is in the rest array.
    let extra = units_of(&[
        ("a", A),
        (
            "m",
            "import { trace } from './a'; function mk() { return { on: true, n: 1 }; } function g() { const s = mk(); trace(0, 'x', s); return s.n; } console.log(g());",
        ),
    ]);
    assert_ne!(verdicts(&extra, "n"), ["T0Mono"], "{}", extra.dump_text());
    // A body that reads `arguments` sees every argument.
    let args = units_of(&[
        ("a", "export function trace(s: any) { if (s.on) console.log(arguments.length); }"),
        (
            "m",
            "import { trace } from './a'; function mk() { return { on: true, n: 1 }; } function g() { const s = mk(); trace(s); return s.n; } console.log(g());",
        ),
    ]);
    assert_ne!(verdicts(&args, "n"), ["T0Mono"], "{}", args.dump_text());
}
