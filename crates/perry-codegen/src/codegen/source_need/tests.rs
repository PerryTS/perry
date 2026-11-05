use std::collections::BTreeMap;

use super::flow::{category_mask, Category, Class3, Ent, Need, Reason};
use perry_hir::Module;

fn lower(src: &str, name: &str) -> Module {
    let parsed = perry_parser::parse_typescript(src, name).expect("parse");
    perry_hir::lower_module(&parsed, name, name).expect("lower")
}

fn run_links(ms: &[(&str, &Module)], links: &[(&str, &str, &str)]) -> Need {
    let modules: Vec<_> = ms.iter().map(|(p, m)| (p.to_string(), *m)).collect();
    let links: BTreeMap<_, _> = links
        .iter()
        .map(|(from, spec, to)| ((from.to_string(), spec.to_string()), to.to_string()))
        .collect();
    Need::compute(&modules, &links, ms[0].0, false, &Default::default())
}

fn run(src: &str) -> (Module, Need) {
    let m = lower(src, "entry.ts");
    let need = run_links(&[("entry.ts", &m)], &[]);
    (m, need)
}

fn fid(m: &Module, name: &str) -> u32 {
    m.functions
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("no function {name}"))
        .id
}

fn class_id(m: &Module, name: &str) -> u32 {
    m.classes.iter().find(|c| c.name == name).expect("class").id
}

fn method_id(m: &Module, class: &str, name: &str) -> u32 {
    let c = m.classes.iter().find(|c| c.name == class).expect("class");
    c.methods
        .iter()
        .chain(&c.static_methods)
        .find(|f| f.name == name)
        .expect("method")
        .id
}

fn class_of(need: &Need, ent: Ent, ignored: u64) -> Class3 {
    need.class_of(need.entity(ent).expect("entity"), ignored)
}

fn strict(need: &Need, ent: Ent) -> Class3 {
    class_of(need, ent, 0)
}

fn known(need: &Need, ent: Ent) -> bool {
    need.mask[need.entity(ent).unwrap()] & category_mask(Category::Known) != 0
}

#[test]
fn a_function_only_called_directly_is_never_a_value() {
    let (m, need) = run("function helper(x: number) { return x * 2; } console.log(helper(2));");
    assert_eq!(
        strict(&need, Ent::Func(0, fid(&m, "helper"))),
        Class3::NeverValue
    );
}

#[test]
fn template_and_string_conversions_are_known_sinks() {
    let (m, need) = run(
        "function a() {} function b() {} function c() {} function d() {}
         const s1 = `${a}`; const s2 = String(b); const s3 = '' + c; const s4 = d.toString();
         console.log(s1.length + s2.length + s3.length + s4.length);",
    );
    for name in ["a", "b", "c", "d"] {
        let e = Ent::Func(0, fid(&m, name));
        assert_eq!(strict(&need, e), Class3::MayStringify, "{name}");
        assert!(known(&need, e), "{name} should reach a known sink");
    }
}

#[test]
fn function_prototype_to_string_call_and_property_keys_are_known_sinks() {
    let (m, need) = run("function viaCall() {} function asKey() {}
         console.log(Function.prototype.toString.call(viaCall).length);
         const o: any = {}; o[asKey as any] = 1; console.log(Object.keys(o).length);");
    for name in ["viaCall", "asKey"] {
        let e = Ent::Func(0, fid(&m, name));
        assert_eq!(strict(&need, e), Class3::MayStringify, "{name}");
        assert!(known(&need, e), "{name}");
    }
}

#[test]
fn a_local_alias_that_is_only_called_reaches_no_sink() {
    let (m, need) = run("function g() { return 1; } const h = g; console.log(h());");
    assert_eq!(strict(&need, Ent::Func(0, fid(&m, "g"))), Class3::NoSink);
}

#[test]
fn values_through_direct_call_parameters_and_returns_are_followed() {
    let (m, need) = run(
        "function id(x: any) { return x; } function pass(x: any) { return x; }
         function kept() {} function dropped() {}
         const s = String(id(kept)); const t = pass(dropped); console.log(s.length, typeof t);",
    );
    assert_eq!(
        strict(&need, Ent::Func(0, fid(&m, "kept"))),
        Class3::MayStringify
    );
    assert_eq!(
        strict(&need, Ent::Func(0, fid(&m, "dropped"))),
        Class3::NoSink
    );
}

#[test]
fn untracked_callees_and_the_heap_are_unknown_sinks() {
    let (m, need) = run(
        "function logged() {} function stored() {} console.log(logged); const o = { f: stored }; console.log(o);",
    );
    for name in ["logged", "stored"] {
        let e = Ent::Func(0, fid(&m, name));
        assert_eq!(strict(&need, e), Class3::MayStringify, "{name}");
        assert!(!known(&need, e), "{name} reaches only unknown sinks");
        // Trusting unknown sinks would drop it: the sensitivity bound.
        let trust = Reason::Export.bit() | category_mask(Category::Unknown);
        assert_ne!(class_of(&need, e, trust), Class3::MayStringify, "{name}");
    }
}

#[test]
fn exports_escape_under_the_strict_rule_only() {
    let (m, need) = run("export function exported() { return 1; } console.log(exported());");
    let e = Ent::Func(0, fid(&m, "exported"));
    assert_eq!(strict(&need, e), Class3::MayStringify);
    assert_ne!(
        class_of(&need, e, Reason::Export.bit()),
        Class3::MayStringify
    );
}

#[test]
fn an_import_resolves_to_the_exporting_function_and_its_parameters() {
    let lib = lower("export function show(f: any) { return `${f}`; }", "lib.ts");
    let entry = lower(
        "import { show } from './lib'; function shown() {} function hidden() {} console.log(show(shown).length); console.log(typeof hidden);",
        "entry.ts",
    );
    let need = run_links(
        &[("entry.ts", &entry), ("lib.ts", &lib)],
        &[("entry.ts", "./lib", "lib.ts")],
    );
    let wp = Reason::Export.bit();
    assert_eq!(
        class_of(&need, Ent::Func(0, fid(&entry, "shown")), wp),
        Class3::MayStringify
    );
    assert!(known(&need, Ent::Func(0, fid(&entry, "shown"))));
    assert_ne!(
        class_of(&need, Ent::Func(0, fid(&entry, "hidden")), wp),
        Class3::MayStringify
    );
}

#[test]
fn classes_follow_instantiation_and_members_live_in_the_heap() {
    let (m, need) = run(
        "class Used { m() { return 1; } } class Unused { n() {} } console.log(new Used().m());",
    );
    assert_eq!(
        strict(&need, Ent::Class(0, class_id(&m, "Used"))),
        Class3::MayStringify
    );
    assert_eq!(
        strict(&need, Ent::Func(0, method_id(&m, "Used", "m"))),
        Class3::MayStringify
    );
    assert_eq!(
        strict(&need, Ent::Class(0, class_id(&m, "Unused"))),
        Class3::NeverValue
    );
}

#[test]
fn a_parent_class_is_reachable_from_its_subclass() {
    let (m, need) = run("class P {} class C extends P {} console.log(String(C).length);");
    assert_eq!(
        strict(&need, Ent::Class(0, class_id(&m, "C"))),
        Class3::MayStringify
    );
    assert_eq!(
        strict(&need, Ent::Class(0, class_id(&m, "P"))),
        Class3::MayStringify
    );
}

#[test]
fn static_this_carries_the_class() {
    let (m, need) =
        run("class S { static name2() { return `${this}`; } } console.log(S.name2().length);");
    let e = Ent::Class(0, class_id(&m, "S"));
    assert_eq!(strict(&need, e), Class3::MayStringify);
    assert!(known(&need, e));
}

#[test]
fn an_immediately_invoked_function_is_never_a_value() {
    let (m, need) = run("(function iife() { console.log('x'); })();");
    let closures: Vec<_> = need
        .entities
        .iter()
        .enumerate()
        .filter(|(_, e)| e.kind == "closure")
        .collect();
    assert_eq!(closures.len(), 1, "{:?}", need.entities);
    assert_eq!(need.class_of(closures[0].0, 0), Class3::NeverValue);
    let _ = m;
}

#[test]
fn numeric_conversion_of_a_function_value_stringifies_it() {
    let (m, need) = run("function f() {} let x: any = f; x++; console.log(x);");
    assert!(known(&need, Ent::Func(0, fid(&m, "f"))));
}

#[test]
fn union_bytes_counts_each_stored_byte_by_its_highest_owner() {
    // A (c) parent [0, 10) contains an (a) child [2, 6); an (a) sibling [10, 14).
    let u = super::union_bytes(16, &[(0, 10, 2), (2, 4, 0), (10, 4, 0)], 3);
    assert_eq!(u, vec![4, 0, 10, 2]);
}

#[test]
fn a_property_write_error_prints_class_source_only() {
    // The runtime's read-only-property TypeError renders a class's source,
    // never a function's.
    let (m, need) = run(
        "function f() {} class C {} const t: any = f; t.x = 1; const u: any = C; u.y = 2; console.log(typeof t, typeof u);",
    );
    assert_ne!(
        strict(&need, Ent::Func(0, fid(&m, "f"))),
        Class3::MayStringify
    );
    assert!(known(&need, Ent::Class(0, class_id(&m, "C"))));
}

#[test]
fn a_dynamic_import_without_known_targets_publishes_every_namespace() {
    let mut entry = lower(
        "const spec: any = 'x'; console.log(typeof spec);",
        "entry.ts",
    );
    entry
        .init
        .push(perry_hir::Stmt::Expr(perry_hir::Expr::DynamicImport {
            paths: vec![],
            arg: Box::new(perry_hir::Expr::String("x".into())),
            options: None,
            byte_offset: 0,
            deferred_error: None,
            synchronous: false,
        }));
    let lib = lower("export function exported() { return 1; }", "lib.ts");
    let need = run_links(&[("entry.ts", &entry), ("lib.ts", &lib)], &[]);
    let e = Ent::Func(1, fid(&lib, "exported"));
    // Escapes even in a closed executable: the namespace object is the heap.
    assert_eq!(
        class_of(&need, e, Reason::Export.bit()),
        Class3::MayStringify
    );
    assert!(need
        .reasons(need.entity(e).unwrap())
        .contains(&"namespace-object"));
}

#[test]
fn an_unnameable_export_binding_keeps_the_module_conservative() {
    let mut m = lower(
        "function kept() {} const v = 1; console.log(v);",
        "entry.ts",
    );
    m.exports.push(perry_hir::Export::Named {
        local: "noSuchBinding".into(),
        exported: "x".into(),
    });
    let need = run_links(&[("entry.ts", &m)], &[]);
    let e = Ent::Func(0, fid(&m, "kept"));
    assert_eq!(
        class_of(&need, e, Reason::Export.bit()),
        Class3::MayStringify
    );
    assert_eq!(need.notes.get("export-local-unresolved"), Some(&1));
}
