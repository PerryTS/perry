use super::*;
use crate::realm_manifest::RealmKind;

macro_rules! entry {
    ($name:literal $(=> $($birth:literal),*)?) => {
        RealmEntry { name: $name, kind: RealmKind::Constructor,
            births: &[$($($birth),*)?], module: None, facts: "{}" }
    };
}
const CATALOG: &[RealmEntry] = &[
    entry!("Object"),
    entry!("Function"),
    entry!("Array"),
    entry!("Error"),
    entry!("TypeError"),
    entry!("RangeError"),
    entry!("SyntaxError"),
    entry!("ReferenceError"),
    entry!("EvalError"),
    entry!("URIError"),
    entry!("AggregateError"),
    entry!("SuppressedError"),
    entry!("String"),
    entry!("Number"),
    entry!("Boolean"),
    entry!("BigInt"),
    entry!("Symbol"),
    entry!("Map"),
    entry!("Set"),
    entry!("RegExp"),
    entry!("Promise" => "Array"),
    entry!("Buffer" => "Uint8Array"),
    entry!("Uint8Array" => "%TypedArray%"),
    entry!("%TypedArray%" => "ArrayBuffer"),
    entry!("ArrayBuffer"),
    entry!("%ArrayIteratorPrototype%" => "%IteratorPrototype%"),
    entry!("%StringIteratorPrototype%" => "%IteratorPrototype%"),
    entry!("%MapIteratorPrototype%"),
    entry!("%SetIteratorPrototype%"),
    entry!("%IteratorPrototype%"),
    entry!("%GeneratorPrototype%"),
    entry!("%AsyncGeneratorPrototype%" => "%AsyncIteratorPrototype%"),
    entry!("%AsyncIteratorPrototype%"),
    entry!("NeverNamed"),
    RealmEntry {
        name: "node:fs",
        kind: RealmKind::Namespace,
        births: &["Buffer", "Error"],
        module: Some("fs"),
        facts: "{}",
    },
];

fn reach(source: &str) -> RealmReach {
    let module = perry_parser::parse_typescript(source, "realm.cjs").unwrap();
    scan(&module, CATALOG)
}

#[test]
fn each_top_fixture_selects_the_full_catalog() {
    let cases = [
        (
            "eval",
            include_str!("realm_fixtures/top_eval.ts"),
            RealmTop::DynamicCode,
        ),
        (
            "Function",
            include_str!("realm_fixtures/top_function.ts"),
            RealmTop::DynamicCode,
        ),
        (
            "vm",
            include_str!("realm_fixtures/top_vm.ts"),
            RealmTop::DynamicCode,
        ),
        (
            "data import",
            include_str!("realm_fixtures/top_data_import.ts"),
            RealmTop::DynamicCode,
        ),
        (
            "escape",
            include_str!("realm_fixtures/top_global_value.ts"),
            RealmTop::GlobalObjectValue,
        ),
        (
            "dynamic key",
            include_str!("realm_fixtures/top_global_key.ts"),
            RealmTop::GlobalObjectValue,
        ),
        (
            "enumeration",
            include_str!("realm_fixtures/top_global_enum.ts"),
            RealmTop::GlobalObjectValue,
        ),
        (
            "with",
            include_str!("realm_fixtures/top_with.ts"),
            RealmTop::With,
        ),
        (
            "require",
            include_str!("realm_fixtures/top_require.ts"),
            RealmTop::DynamicModule,
        ),
        (
            "import",
            include_str!("realm_fixtures/top_import.ts"),
            RealmTop::DynamicModule,
        ),
    ];
    for (name, source, reason) in cases {
        let r = reach(source);
        assert!(r.top.contains(&reason), "{name}: {r:?}");
        assert_eq!(r.names.len(), CATALOG.len(), "{name}: {r:?}");
        assert!(r.names.contains("NeverNamed"));
    }
}

#[test]
fn every_global_spelling_escapes_but_static_keys_do_not() {
    for spelling in ["globalThis", "global", "window", "self", "frames"] {
        assert!(
            reach(&format!("consume({spelling});")).is_full(),
            "{spelling}"
        );
        assert!(
            !reach(&format!("{spelling}.Map; {spelling}['Set'];")).is_full(),
            "{spelling}"
        );
    }
}

#[test]
fn only_a_global_any_member_patch_selects_top() {
    let mut patched = crate::PatchedBuiltins::default();
    patched.insert(("Map".into(), crate::patched_builtins::ANY_MEMBER.into()));
    let mut r = RealmReach::default();
    r.program_signals(&patched);
    r.close(CATALOG);
    assert!(!r.is_full());
    for spelling in ["globalThis", "global", "self", "window", "frames"] {
        let mut p = patched.clone();
        p.insert((spelling.into(), crate::patched_builtins::ANY_MEMBER.into()));
        let mut r = RealmReach::default();
        r.program_signals(&p);
        r.close(CATALOG);
        assert_eq!(r.top, BTreeSet::from([RealmTop::GlobalAnyMember]));
        assert_eq!(r.names.len(), CATALOG.len());
    }
}

#[test]
fn all_non_top_features_stay_partial_and_include_their_births() {
    let r = reach(include_str!("realm_fixtures/non_top.ts"));
    assert!(!r.is_full(), "{r:?}");
    assert!(!r.names.contains("NeverNamed"));
    for name in [
        "Map",
        "RegExp",
        "Promise",
        "Array",
        "Buffer",
        "Uint8Array",
        "%TypedArray%",
        "ArrayBuffer",
        "node:fs",
        "%GeneratorPrototype%",
        "%AsyncIteratorPrototype%",
    ] {
        assert!(r.names.contains(name), "missing {name}: {r:?}");
    }
}

#[test]
fn local_names_and_types_are_not_global_references() {
    let r = reach("function f(Map, globalThis, require) { globalThis[key]; require(key); return Map; } type X = Set;");
    assert!(!r.is_full(), "{r:?}");
    assert!(!r.names.contains("Map"));
    assert!(!r.names.contains("Set"));
}

#[test]
fn constant_code_is_program_code_and_birth_closure_is_transitive() {
    let r = reach("Function('return Buffer')(); eval('Map');");
    assert!(!r.is_full(), "{r:?}");
    for name in ["Buffer", "Uint8Array", "%TypedArray%", "ArrayBuffer", "Map"] {
        assert!(r.names.contains(name), "{name}: {r:?}");
    }
}

#[test]
fn merged_modules_close_new_edges_and_apply_program_signals() {
    let mut a = reach("0;");
    let b = reach("require('node:fs');");
    a.merge(&b);
    a.close(CATALOG);
    assert!(a.names.contains("ArrayBuffer"));
    let mut patched = crate::PatchedBuiltins::default();
    patched
        .dynamic_code_reach
        .record_global("other module".into());
    a.program_signals(&patched);
    a.close(CATALOG);
    assert_eq!(a.names.len(), CATALOG.len());
}

#[test]
fn constant_global_keys_and_module_names_are_not_top() {
    let r=reach("const a='Ma'; const b=a+'p'; globalThis[b]; globalThis[42]; globalThis[undefined]; const fs='node:fs'; require(fs);");
    assert!(!r.is_full(), "{r:?}");
    assert!(r.names.contains("Map"));
    assert!(r.names.contains("node:fs"));
}

#[test]
fn lowered_global_values_and_static_member_receivers_are_distinct() {
    for (expr, full) in [
        (crate::Expr::GlobalThisExpr, true),
        (
            crate::Expr::IndexGet {
                byte_offset: 0,
                byte_end: 0,
                object: Box::new(crate::Expr::GlobalThisExpr),
                index: Box::new(crate::Expr::String("Map".into())),
            },
            false,
        ),
        (
            crate::Expr::IndexGet {
                byte_offset: 0,
                byte_end: 0,
                object: Box::new(crate::Expr::GlobalThisExpr),
                index: Box::new(crate::Expr::GlobalThisExpr),
            },
            true,
        ),
    ] {
        let mut module = crate::Module::new("realm");
        module.init.push(crate::Stmt::Expr(expr));
        let mut r = RealmReach::default();
        scan_lowered(&module, CATALOG, &mut r);
        assert_eq!(r.is_full(), full, "{r:?}");
        if !full {
            assert!(r.names.contains("Map"));
        }
    }
}

#[test]
fn non_top_fixture_remains_partial_after_hir_lowering() {
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(|| {
            let source = include_str!("realm_fixtures/non_top.ts");
            let module = perry_parser::parse_typescript(source, "realm.cjs").unwrap();
            let lowered = crate::lower_module(&module, "realm", "realm.cjs").unwrap();
            let mut r = scan(&module, CATALOG);
            scan_lowered(&lowered, CATALOG, &mut r);
            assert!(!r.is_full(), "{r:?}");
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn require_aliases_keep_static_imports_partial_and_dynamic_calls_top() {
    let partial = reach("const load=require; const next=load; next('node:fs');");
    assert!(!partial.is_full(), "{partial:?}");
    assert!(partial.names.contains("node:fs"));
    let dynamic = reach("const load=require; load(name);");
    assert!(dynamic.top.contains(&RealmTop::DynamicModule));
    assert!(!reach("function f(require) { const load=require; load(name); }").is_full());
}

#[test]
fn type_only_bases_and_tagged_template_quasis_add_no_builtin() {
    let r = reach("interface X extends Map {} tag`literal`; ");
    assert!(!r.names.contains("Map"));
    assert!(!r.names.contains("String"));
    assert!(!r.is_full());
}

#[test]
fn chained_global_aliases_escape_only_when_the_object_value_is_read() {
    for source in [
        "consume(globalThis.self);",
        "consume(globalThis['globalThis']);",
    ] {
        assert!(
            reach(source).top.contains(&RealmTop::GlobalObjectValue),
            "{source}"
        );
    }
    for source in [
        "globalThis.self.Map;",
        "globalThis.global['Set'];",
        "globalThis.self=1;",
        "delete globalThis.self;",
    ] {
        assert!(!reach(source).is_full(), "{source}");
    }
}

#[test]
fn global_patch_fixture_reaches_top_through_the_program_patch_signal() {
    let fixture: Vec<(String, String)> =
        serde_json::from_str(include_str!("realm_fixtures/top_global_any_member.json")).unwrap();
    let mut patched = crate::PatchedBuiltins::default();
    for patch in fixture {
        patched.insert(patch);
    }
    let mut r = RealmReach::default();
    r.program_signals(&patched);
    r.close(CATALOG);
    assert_eq!(r.top, BTreeSet::from([RealmTop::GlobalAnyMember]));
    assert_eq!(r.names.len(), CATALOG.len());
}
