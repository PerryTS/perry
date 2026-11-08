use super::*;
use crate::realm_manifest::RealmKind;

macro_rules! entry {
    ($name:literal $(=> $($birth:literal),*)?) => {
        RealmEntry { name: $name, kind: RealmKind::Constructor,
            births: &[$($($birth),*)?], module: None, facts: "{}", effects: &[] }
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
        effects: &[],
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
fn restricting_drops_top_and_closes_the_given_names() {
    // `with` makes R top, and the scan closes it to the whole catalog.
    let mut r = reach("new Map(); with ({}) { x; }");
    assert!(r.is_full(), "{r:?}");
    assert_eq!(r.names.len(), CATALOG.len());
    r.restrict_to(["Buffer".to_string()]);
    assert!(!r.is_full());
    r.close(CATALOG);
    // The given names and their births; nothing else.
    let expected = ["Buffer", "Uint8Array", "%TypedArray%", "ArrayBuffer"];
    assert_eq!(
        r.names,
        expected
            .iter()
            .map(|n| n.to_string())
            .collect::<BTreeSet<_>>()
    );
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
                element_offset: 0,
                byte_offset: 0,
                byte_end: 0,
                object: Box::new(crate::Expr::GlobalThisExpr),
                index: Box::new(crate::Expr::String("Map".into())),
            },
            false,
        ),
        (
            crate::Expr::IndexGet {
                element_offset: 0,
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

#[test]
fn create_require_results_are_require_for_reach() {
    // A literal call through a const alias, or in place, adds the module.
    for source in [
        "import { createRequire } from \"node:module\"; const req = createRequire(__filename); req(\"fs\");",
        "const m = require(\"module\"); const req = m.createRequire(__filename); req(\"node:fs\");",
        "import { createRequire } from \"node:module\"; createRequire(__filename)(\"fs\");",
        "import { createRequire } from \"node:module\"; const req = createRequire(__filename); const again = req; again(\"fs\");",
    ] {
        let partial = reach(source);
        assert!(!partial.is_full(), "{source}: {partial:?}");
        assert!(partial.names.contains("node:fs"), "{source}: {partial:?}");
    }
    // A non-literal argument is top, exactly as for `require`.
    let dynamic = reach(
        "import { createRequire } from \"node:module\"; const req = createRequire(__filename); req(name);",
    );
    assert!(
        dynamic.top.contains(&RealmTop::DynamicModule),
        "{dynamic:?}"
    );
    // A made require function that escapes (not a const alias, not called in
    // place) has calls the scan cannot see: top.
    for source in [
        "import { createRequire } from \"node:module\"; let req = createRequire(__filename); req(\"fs\");",
        "import { createRequire } from \"node:module\"; use(createRequire(__filename));",
    ] {
        assert!(reach(source).top.contains(&RealmTop::DynamicModule), "{source}");
    }
    // Without createRequire the same call names nothing and stays partial.
    let plain = reach("const req = make(); req(\"fs\");");
    assert!(
        !plain.is_full() && !plain.names.contains("node:fs"),
        "{plain:?}"
    );
}

#[test]
fn get_builtin_module_is_require_for_reach() {
    let partial = reach("const fs = process.getBuiltinModule(\"node:fs\");");
    assert!(!partial.is_full(), "{partial:?}");
    assert!(partial.names.contains("node:fs"), "{partial:?}");
    let dynamic = reach("process.getBuiltinModule(name);");
    assert!(
        dynamic.top.contains(&RealmTop::DynamicModule),
        "{dynamic:?}"
    );
    // A parameter called `process` can be the real one (`f(process)`): a
    // `getBuiltinModule` member is matched on any object.
    let local = reach("function f(process) { process.getBuiltinModule(name); }");
    assert!(local.top.contains(&RealmTop::DynamicModule), "{local:?}");
    // Through a global object, as test_gap_worker_zlib_stream.ts's helper does.
    for source in [
        "const s = globalThis.process.getBuiltinModule(\"node:fs\");",
        "const s = (globalThis as any).process.getBuiltinModule(\"node:fs\");",
    ] {
        let global = reach(source);
        assert!(!global.is_full(), "{source}: {global:?}");
        assert!(global.names.contains("node:fs"), "{source}: {global:?}");
    }
    let global_dynamic = reach("(globalThis as any).process.getBuiltinModule(name);");
    assert!(
        global_dynamic.top.contains(&RealmTop::DynamicModule),
        "{global_dynamic:?}"
    );
    // The same holds through a local `globalThis`.
    let shadowed = reach("function f(globalThis) { globalThis.process.getBuiltinModule(name); }");
    assert!(
        shadowed.top.contains(&RealmTop::DynamicModule),
        "{shadowed:?}"
    );
}

/// Each source loads node:fs with a literal: the scan must add it and stay
/// partial. The same source with a non-literal argument must be top.
fn assert_loads_fs(form: &str, literal: &str) {
    let partial = reach(literal);
    assert!(!partial.is_full(), "{form}: {literal}: {partial:?}");
    assert!(
        partial.names.contains("node:fs"),
        "{form}: {literal}: {partial:?}"
    );
    let dynamic_source = literal
        .replace("\"node:fs\"", "name")
        .replace("\"fs\"", "name");
    assert_ne!(dynamic_source, literal, "{form}: fixture has no literal");
    let dynamic = reach(&dynamic_source);
    assert!(
        dynamic.top.contains(&RealmTop::DynamicModule),
        "{form}: {dynamic_source}: {dynamic:?}"
    );
}

fn assert_escapes(form: &str, source: &str) {
    let r = reach(source);
    assert!(
        r.top.contains(&RealmTop::DynamicModule),
        "{form}: {source}: {r:?}"
    );
}

#[test]
fn optional_chain_get_builtin_module_loads_the_module() {
    assert_loads_fs(
        "optional call",
        "const fs = process.getBuiltinModule?.(\"node:fs\");",
    );
    assert_loads_fs(
        "optional member",
        "const fs = globalThis.process?.getBuiltinModule(\"node:fs\");",
    );
    assert_loads_fs(
        "optional member and call",
        "const fs = globalThis.process?.getBuiltinModule?.(\"node:fs\");",
    );
}

#[test]
fn optional_call_of_a_require_loads_the_module() {
    assert_loads_fs(
        "optional createRequire result",
        "import { createRequire } from \"node:module\"; const req = createRequire(__filename); req?.(\"fs\");",
    );
}

#[test]
fn renamed_create_require_import_is_resolved_by_binding() {
    assert_loads_fs(
        "renamed import",
        "import { createRequire as cr } from \"node:module\"; const req = cr(__filename); req(\"fs\");",
    );
    assert_loads_fs(
        "renamed import called in place",
        "import { createRequire as cr } from \"node:module\"; cr(__filename)(\"fs\");",
    );
    assert_escapes(
        "renamed import result escapes",
        "import { createRequire as cr } from \"node:module\"; use(cr(__filename));",
    );
}

#[test]
fn destructured_create_require_is_resolved_by_binding() {
    assert_loads_fs(
        "destructured require(\"module\")",
        "const { createRequire: cr } = require(\"node:module\"); const req = cr(__filename); req(\"fs\");",
    );
}

#[test]
fn imported_process_default_loads_the_module() {
    assert_loads_fs(
        "default import",
        "import proc from \"node:process\"; proc.getBuiltinModule(\"node:fs\");",
    );
    assert_loads_fs(
        "namespace import",
        "import * as proc from \"node:process\"; proc.getBuiltinModule(\"node:fs\");",
    );
}

#[test]
fn imported_get_builtin_module_loads_the_module() {
    assert_loads_fs(
        "named import",
        "import { getBuiltinModule } from \"node:process\"; getBuiltinModule(\"node:fs\");",
    );
    assert_loads_fs(
        "renamed named import",
        "import { getBuiltinModule as load } from \"node:process\"; load(\"node:fs\");",
    );
    assert_loads_fs(
        "destructured",
        "const { getBuiltinModule } = process; getBuiltinModule(\"node:fs\");",
    );
    assert_loads_fs(
        "const alias",
        "const load = process.getBuiltinModule; load(\"node:fs\");",
    );
}

#[test]
fn computed_get_builtin_module_loads_the_module() {
    assert_loads_fs(
        "computed key",
        "process[\"getBuiltinModule\"](\"node:fs\");",
    );
    assert_loads_fs(
        "constant computed key",
        "const k = \"getBuiltinModule\"; process[k](\"node:fs\");",
    );
}

#[test]
fn create_require_result_used_as_a_value_is_top() {
    let head =
        "import { createRequire } from \"node:module\"; const req = createRequire(__filename);";
    assert_escapes("map argument", &format!("{head} [\"fs\"].map(req);"));
    assert_escapes("helper argument", &format!("{head} helper(req);"));
    assert_escapes("stored", &format!("{head} const o = {{ load: req }};"));
    assert_escapes("shorthand", &format!("{head} const o = {{ req }};"));
    assert_escapes("call", &format!("{head} req.call(null, \"fs\");"));
    assert_escapes("exported", "import { createRequire } from \"node:module\"; export const req = createRequire(__filename);");
    assert_escapes("export list", &format!("{head} export {{ req }};"));
    assert_escapes(
        "maker as a value",
        "import { createRequire } from \"node:module\"; helper(createRequire);",
    );
}

#[test]
fn require_used_as_a_value_is_top() {
    assert_escapes(
        "alias map argument",
        "const req = require; [\"fs\"].map(req);",
    );
    assert_escapes("alias helper argument", "const req = require; helper(req);");
    assert_escapes("free require argument", "helper(require);");
    assert_escapes("let alias", "let req = require; req(\"fs\");");
    assert_escapes(
        "getBuiltinModule value",
        "[\"fs\"].map(process.getBuiltinModule);",
    );
    // Uses that do not let the loader escape stay partial.
    for source in [
        "const p = require.resolve(\"node:fs\"); require(\"node:fs\");",
        "if (typeof require === \"function\" && require.main === module) require(\"node:fs\");",
        "const req = require; req(\"fs\");",
        "if (typeof process.getBuiltinModule === \"function\") process.getBuiltinModule(\"fs\");",
    ] {
        let r = reach(source);
        assert!(!r.is_full(), "{source}: {r:?}");
        assert!(r.names.contains("node:fs"), "{source}: {r:?}");
    }
}

#[test]
fn an_ambient_require_declaration_keeps_require_the_loader() {
    // HIR lowering erases `declare function require` (#8447) and `declare
    // var`; the scan must read the same: an ES module still loads node:fs.
    assert_loads_fs(
        "declare function require",
        "declare function require(id: string): any; const fs = require(\"fs\"); export const keep = 1;",
    );
    assert_loads_fs(
        "exported declare function require",
        "export declare function require(id: string): any; const fs = require(\"fs\");",
    );
    assert_loads_fs(
        "declare var require",
        "declare var require: any; const fs = require(\"fs\"); export const keep = 1;",
    );
    // A require with a body is a program binding: it loads nothing.
    let r = reach(
        "function require(id: string): any { return id; } const fs = require(\"fs\"); export const keep = 1;",
    );
    assert!(!r.names.contains("node:fs"), "{r:?}");
    assert!(!r.top.contains(&RealmTop::DynamicModule), "{r:?}");
}
