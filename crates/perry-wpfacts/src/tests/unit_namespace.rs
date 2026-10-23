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
    assert!(!x.readonly && x.kind.top, "{}", pf.dump_text());
    // A new key is beyond the contract: C breaks.
    let m =
        "import { C } from './a'; (C as any).Z = 1; function g() { return C.Y; } console.log(g());";
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
    // effect (decision 17), its new key breaks the inner layout (census).
    let u = "import { get } from './a'; const t = get(2).targetRange; (t as any).facts = 'z'; function k() { return t.range; } console.log(k());";
    let pf = units_of(&[("a", a), ("u", u)]);
    let inner = pf
        .templates
        .iter()
        .find(|t| t.fields.iter().any(|f| f.key == "facts"))
        .unwrap_or_else(|| panic!("{}", pf.dump_text()));
    let f = inner.fields.iter().find(|f| f.key == "facts").unwrap();
    assert!(!f.readonly && f.kind.top, "{}", pf.dump_text());
    let u = "import { get } from './a'; const t = get(2).targetRange; (t as any).extra = 1; function k() { return t.range; } console.log(k());";
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
    let a = "import { use } from './b';\nexport function get(n: number) { const inner = { deep: { facts: n } }; use(inner); return { t: inner }; }";
    let b = "export function use(x: any) { x.deep.facts = 'z'; }";
    let u =
        "import { get } from './a'; function k() { return get(2).t.deep.facts; } console.log(k());";
    let pf = units_of(&[("a", a), ("b", b), ("u", u)]);
    let deep = pf
        .templates
        .iter()
        .find(|t| t.fields.iter().any(|f| f.key == "facts"))
        .unwrap_or_else(|| panic!("{}", pf.dump_text()));
    let f = deep.fields.iter().find(|f| f.key == "facts").unwrap();
    assert!(!f.readonly && f.kind.top, "{}", pf.dump_text());
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
