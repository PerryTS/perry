use super::*;
use perry_hir::realm_manifest::{RealmEntry, RealmKind, RealmOwn, RealmOwnValue};
use perry_hir::realm_reach_index::RealmIndex;

#[test]
fn holders_select_exactly_the_logical_members() {
    let index = RealmIndex::new(crate::realm_manifest::REALM);
    for source in [
        "console.log('hi');",
        "const B: any = Buffer; console.log(typeof B.alloc, typeof B.prototype.readUInt8);",
        "console.log(Object.getOwnPropertyNames(Buffer.prototype).length, Object.keys(Error).length);",
        "console.log(Buffer); console.log(TypeError);",
        "for (const k in Promise) console.log(k); console.log(JSON.stringify(Object.getOwnPropertyDescriptors(Set)));",
        "function f(o,k) { o[k]; } Buffer;",
        "'x'.constructor.fromCharCode(65);",
        "const b = Buffer.alloc(2); console.log(b.readUInt8(0), new URL('http://a/b').pathname, [1].map(x => x).length, new Set([1]).size);",
    ] {
        let module = perry_parser::parse_typescript(source, "selection.ts").unwrap();
        let reach = perry_hir::realm_reach::scan(&module, &index);
        assert!(!reach.is_full(), "{source}");
        let plan = RealmPlan::from_reach(&reach).unwrap();
        let mut omitted = 0;
        for (name, cell) in &plan.cells {
            let row = index.last(name).unwrap();
            let original: RealmDeclaration = serde_json::from_value(row.facts().clone()).unwrap();
            if row.runtime_layout_holder() {
                assert_eq!(cell.properties.len(), original.properties.len(), "{name}");
                continue;
            }
            for property in &original.properties {
                let enumerable = property.attributes & 2 != 0;
                let get = property.get.as_ref().filter(|n| reach.keeps_member(&index, row, &property.key, n, enumerable));
                let set = property.set.as_ref().filter(|n| reach.keeps_member(&index, row, &property.key, n, enumerable));
                let value = match &property.value {
                    Some(RealmValue::Reference(n) | RealmValue::RawReference(n)) => n == "%Global%" || reach.keeps_member(&index, row, &property.key, n, enumerable),
                    Some(_) => true,
                    None => false,
                };
                let selected = cell.properties.iter().find(|p| p.key == property.key && p.symbol == property.symbol);
                assert_eq!(selected.is_some(), value || get.is_some() || set.is_some(), "{source}: {name}.{}", property.key);
                if let Some(selected) = selected {
                    assert_eq!(selected.get.as_ref(), get);
                    assert_eq!(selected.set.as_ref(), set);
                } else {
                    omitted += 1;
                }
            }
        }
        assert!(omitted > 0, "selection was not exercised: {source}");
    }
}

#[test]
fn f7_plan_keeps_the_bind_time_constructor_prototype() {
    const OWN: &[RealmOwn] = &[RealmOwn {
        key: "prototype",
        symbol: false,
        value: RealmOwnValue::Reference("Ctor.prototype"),
    }];
    let index = RealmIndex::new(&[
        RealmEntry {
            name: "%Global%",
            kind: RealmKind::Namespace,
            births: &[],
            module: None,
            facts: "{}",
            effects: &[],
            own: &[],
            proto: None,
        },
        RealmEntry {
            name: "Ctor",
            kind: RealmKind::Constructor,
            births: &["Ctor.prototype"],
            module: None,
            facts: r#"{"constructor":{}}"#,
            effects: &[],
            own: OWN,
            proto: None,
        },
        RealmEntry {
            name: "Ctor.prototype",
            kind: RealmKind::Prototype,
            births: &[],
            module: None,
            facts: "{}",
            effects: &[],
            own: &[],
            proto: None,
        },
    ]);
    let mut reach = RealmReach::default();
    reach.own("Ctor");
    reach.close(&index);
    let declarations = [
        r#"{"name":"%Global%","kind":"namespace","header":{"gc_type":2,"reserved":0}}"#,
        r#"{"name":"Ctor","kind":"constructor","header":{"gc_type":2,"reserved":2048},"properties":[{"key":"prototype","symbol":false,"attributes":0,"value":{"kind":"reference","value":"Ctor.prototype"}}],"own_key_order":["prototype"]}"#,
        r#"{"name":"Ctor.prototype","kind":"prototype","header":{"gc_type":2,"reserved":0}}"#,
    ].iter().map(|text| serde_json::from_str(text).unwrap()).collect();
    let plan = RealmPlan::from_declarations(&reach, declarations, &index).unwrap();
    assert!(reach.structure.contains("Ctor.prototype"));
    assert_eq!(plan.cells["Ctor"].properties.len(), 1);
    assert_eq!(plan.cells["Ctor"].properties[0].key, "prototype");
    assert!(plan.cells.contains_key("Ctor.prototype"));
}

#[test]
fn pure_console_and_unnamed_function_roots_have_structural_chains() {
    let index = RealmIndex::new(crate::realm_manifest::REALM);
    for (roots, own, expected) in [
        (&["console", "console.log"][..], true, (10, 5, 5)),
        (&["Function"][..], false, (5, 0, 5)),
    ] {
        let mut reach = RealmReach::default();
        for name in roots {
            reach.names.insert((*name).into());
            if own {
                reach.own(name);
            } else {
                reach.chain_roots.insert((*name).into());
            }
        }
        reach.close(&index);
        assert_eq!(
            (reach.names.len(), reach.whole.len(), reach.structure.len()),
            expected
        );
        assert!(!reach.names.contains("Object"));
        assert!(!reach.whole.contains("Function"));
        if own {
            assert!(!reach.names.contains("Function"));
        }
        for name in ["Object.prototype", "Function.prototype"] {
            assert!(reach.structure.contains(name));
            if own {
                let row = index.last(name).unwrap();
                for (key, symbol, value) in row.members() {
                    assert!(
                        !reach.keeps_member(
                            &index,
                            row,
                            key,
                            value,
                            row.member_enumerable(key, symbol)
                        ),
                        "{name}.{key}"
                    );
                }
            }
        }
        let once = reach.clone();
        reach.close(&index);
        assert_eq!(reach, once);
        RealmPlan::from_reach(&reach).unwrap();
    }
}

#[test]
fn member_selection_projects_the_header_from_the_selected_shape() {
    let index = RealmIndex::new(crate::realm_manifest::REALM);
    let mut reach = RealmReach::default();
    reach.names.insert("RegExp.prototype".into());
    reach.close(&index);
    let original: RealmDeclaration =
        serde_json::from_value(index.last("RegExp.prototype").unwrap().facts().clone()).unwrap();
    assert!(!original.properties.is_empty());
    assert_ne!(
        original.header.as_ref().unwrap().reserved & perry_abi::OBJ_FLAG_HAS_DESCRIPTORS,
        0
    );
    let plan = RealmPlan::from_reach(&reach).unwrap();
    let selected = &plan.cells["RegExp.prototype"];
    assert!(selected.properties.is_empty());
    assert_eq!(
        selected.header.as_ref().unwrap().reserved & perry_abi::OBJ_FLAG_HAS_DESCRIPTORS,
        0
    );
    crate::codegen::realm_image::header::object_word(selected, selected.header.as_ref().unwrap())
        .unwrap();
    let mut declarations: Vec<RealmDeclaration> = index
        .rows()
        .map(|row| serde_json::from_value(row.facts().clone()).unwrap())
        .collect();
    let bad = declarations
        .iter_mut()
        .find(|row| row.name == "RegExp.prototype")
        .unwrap();
    bad.header.as_mut().unwrap().reserved ^= perry_abi::OBJ_FLAG_HAS_DESCRIPTORS;
    let error = RealmPlan::from_declarations(&reach, declarations, &index).unwrap_err();
    assert!(
        error.contains("RegExp.prototype") && error.contains("disagrees"),
        "{error}"
    );
}

#[test]
fn hasinstance_reads_slots_without_observing_their_values() {
    let index = RealmIndex::new(crate::realm_manifest::REALM);
    let mut r = RealmReach::default();
    r.chain_roots
        .extend(["Object.prototype".into(), "Function.prototype".into()]);
    r.receiver_keys.extend(
        perry_hir::realm_manifest::RealmProtocol::HasInstance
            .receiver_keys()
            .iter()
            .map(|k| k.to_string()),
    );
    r.close(&index);
    eprintln!(
        "HasInstance R={} WHOLE={} STRUCTURE={}",
        r.names.len(),
        r.whole.len(),
        r.structure.len()
    );
    assert!(!r.names.contains("Object"));
    assert!(!r.names.contains("Function"));
    assert!(r.structure.contains("Function.prototype"));
    let once = r.clone();
    r.close(&index);
    assert_eq!(r, once);
}

fn scanned(source: &str) -> RealmReach {
    let index = RealmIndex::new(crate::realm_manifest::REALM);
    let module = perry_parser::parse_typescript(source, "events.cjs").unwrap();
    perry_hir::realm_reach::scan(&module, &index)
}

#[test]
fn realm_kinds_enter_only_by_program_events() {
    for (source, object, function) in [
        ("({});", true, false),
        ("function f() {}", true, true),
        ("()=>1;", false, true),
        ("const m = Math.max;", false, true),
        ("Math.max(1,2);", false, false),
    ] {
        let r = scanned(source);
        assert_eq!(
            r.chain_roots.contains("Object.prototype"),
            object,
            "{source}: {r:?}"
        );
        // Closing follows a held callable's chain; syntax/value events must
        // additionally record their root before that closure.
        if function {
            assert!(
                r.chain_roots.contains("Function.prototype"),
                "{source}: {r:?}"
            );
        }
        for ctor in ["Object", "Function"] {
            assert!(!r.names.contains(ctor), "{source}: {ctor}: {r:?}");
        }
    }
    for (source, name) in [
        ("[];", "Array"),
        ("/a/;", "RegExp"),
        ("class A extends Error {}", "Error"),
    ] {
        assert!(scanned(source).names.contains(name), "{source}: {name}");
    }
    let parsed = scanned("JSON.parse('{}');");
    assert!(parsed.chain_roots.contains("Object.prototype"));
    for primitive in ["String", "Number", "Boolean", "BigInt", "Symbol"] {
        assert!(
            !scanned("1; 's'; true; 1n;").names.contains(primitive),
            "{primitive}"
        );
    }
    for source in [
        "String(1);",
        "Number('2');",
        "Symbol('s');",
        "JSON.parse('1');",
    ] {
        let r = scanned(source);
        for primitive in ["String", "Number", "Boolean", "BigInt", "Symbol"] {
            if !source.starts_with(primitive) {
                assert!(!r.own_names.contains(primitive), "{source}: {primitive}");
            }
        }
    }
}

#[test]
fn hello_has_no_constructor_cells_or_kept_prototype_members() {
    let r = scanned("console.log('hello');");
    assert!(!r.names.contains("Object"), "{r:?}");
    assert!(!r.names.contains("Function"), "{r:?}");
    let plan = RealmPlan::from_reach(&r).unwrap();
    for name in ["Object.prototype", "Function.prototype"] {
        assert!(r.structure.contains(name), "{r:?}");
        let row = RealmIndex::new(crate::realm_manifest::REALM);
        let holder = row.last(name).unwrap();
        for (key, symbol, value) in holder.members() {
            assert!(
                !r.keeps_member(
                    &row,
                    holder,
                    key,
                    value,
                    holder.member_enumerable(key, symbol)
                ),
                "{name}.{key}"
            );
            assert!(
                !plan.cells[name]
                    .properties
                    .iter()
                    .any(|p| p.key == key && p.symbol == symbol),
                "{name}.{key}"
            );
        }
        // R4 keeps non-reference primitives in the selected shape.
        assert!(plan.cells[name].properties.iter().all(|p| matches!(
            p.value,
            Some(
                RealmValue::Undefined
                    | RealmValue::Null
                    | RealmValue::Boolean(_)
                    | RealmValue::NumberBits(_)
                    | RealmValue::Int32(_)
                    | RealmValue::String(_)
            )
        )));
    }
    assert_eq!(
        (r.names.len(), r.whole.len(), r.structure.len()),
        (10, 5, 5)
    );
}

#[test]
fn realm_protocol_and_constructor_naming_witnesses() {
    for source in [
        "(()=>1).constructor;",
        "Function.prototype.constructor;",
        "Object.getPrototypeOf(console).constructor;",
    ] {
        let r = scanned(source);
        assert!(
            r.whole.contains(if source.starts_with("Object") {
                "Object"
            } else {
                "Function"
            }),
            "{source}: {r:?}"
        );
    }
    let r = scanned("`${console}`;");
    assert!(r.whole.contains("Object.prototype.toString"), "{r:?}");
    assert!(!r.names.contains("Object"), "{r:?}");
    let r = scanned("function test(x,F) { return x instanceof F; }");
    assert!(!r.whole.contains("Function"), "{r:?}");
    assert!(r.receiver_keys.contains("prototype"));
    assert!(r.receiver_keys.contains("@@hasInstance"));
    let r = scanned("console.hasOwnProperty('x');");
    assert!(
        r.own_names.contains("Object.prototype.hasOwnProperty"),
        "{r:?}"
    );
    let r = scanned("console.log({});");
    assert!(r.whole.contains("Object"), "{r:?}");
}

#[test]
fn eager_url_allocations_own_their_layouts_without_constructor_bodies() {
    let index = RealmIndex::new(crate::realm_manifest::REALM);
    for (owner, components) in [
        ("URL", true),
        ("URL.parse", true),
        ("URL.prototype.href.<set>", true),
        ("URL.prototype.search.<set>", false),
        ("node:url.pathToFileURL", true),
    ] {
        let mut r = RealmReach::default();
        r.own(owner);
        r.close(&index);
        let plan = RealmPlan::from_reach(&r).unwrap();
        assert!(
            plan.runtime_layouts
                .contains_key("%Layout:URL_SEARCH_PARAMS%"),
            "{owner}: {r:?}"
        );
        if components {
            assert!(
                plan.runtime_layouts.contains_key("%Layout:URL_COMPONENTS%"),
                "{owner}: {r:?}"
            );
        }
    }
}

#[test]
fn unnamed_constructor_keeps_prototype_reads_structural() {
    let index = RealmIndex::new(crate::realm_manifest::REALM);
    let mut r = RealmReach::default();
    r.names.insert("Function".into());
    r.receiver_keys.extend(
        perry_hir::realm_manifest::RealmProtocol::HasInstance
            .receiver_keys()
            .iter()
            .map(|k| k.to_string()),
    );
    r.close(&index);
    assert!(r.structure.contains("Function"), "{r:?}");
    assert!(r.structure.contains("Function.prototype"), "{r:?}");
    assert!(!r.names.contains("Object"), "{r:?}");
    let plan = RealmPlan::from_reach(&r).unwrap();
    assert!(plan.cells["Function"]
        .properties
        .iter()
        .any(|p| p.key == "prototype"));
    let once = r.clone();
    r.close(&index);
    assert_eq!(r, once);
}

#[test]
fn literal_and_async_births_supply_their_instance_chains() {
    for (source, proto) in [
        ("[];", "Array.prototype"),
        ("/a/;", "RegExp.prototype"),
        ("async ()=>1;", "Promise.prototype"),
        ("new URL('http://a');", "URL.prototype"),
    ] {
        let r = scanned(source);
        assert!(r.chain_roots.contains(proto), "{source}: {r:?}");
    }
    let r = scanned("console.log(/a/);");
    assert!(
        r.whole.contains("RegExp.prototype"),
        "a declared read sees the instance chain: {r:?}"
    );
}

#[test]
fn startup_path_hook_births_the_global_from_its_emitted_input() {
    assert_eq!(crate::startup_global_birth(&[]), None);
    let paths = vec![("/app/.next/server/chunk.js".into(), "chunk".into())];
    let birth = crate::startup_global_birth(&paths).unwrap();
    assert_eq!(birth, "js_globalthis_seed_async_local_storage");
    let index = RealmIndex::new(crate::realm_manifest::REALM);
    for unknown in [false, true] {
        let mut r = RealmReach::default();
        r.any_receiver_key = unknown;
        r.exec(birth);
        r.close(&index);
        assert!(
            !r.names.contains("%Global%"),
            "an operation does not expose its holder: {r:?}"
        );
        assert!(!r.whole.contains("%Global%"));
        for layout in [
            "IMMEDIATE",
            "ITERATOR_RESULT_DONE_FIRST",
            "RECORD_V026",
            "TIMEOUT",
            "URL_COMPONENTS",
            "URL_SEARCH_PARAMS",
        ] {
            assert!(!r.names.contains(&format!("%Layout:{layout}%")), "{r:?}");
        }
        assert!(r.structure.contains("node:async_hooks.AsyncLocalStorage"));
        assert!(r.names.contains("%Layout:GLOBAL_OBJECT%"));
        RealmPlan::from_reach(&r).unwrap();
    }
}

#[test]
fn fresh_callable_and_object_bodies_declare_instance_births() {
    let index = RealmIndex::new(crate::realm_manifest::REALM);
    for (owner, kind, proto) in [
        ("Function.prototype.bind", "Function", "Function.prototype"),
        ("node:sys.promisify", "Function", "Function.prototype"),
        ("node:sys.callbackify", "Function", "Function.prototype"),
        (
            "node:module.createRequire",
            "Function",
            "Function.prototype",
        ),
        ("Promise.withResolvers", "Function", "Function.prototype"),
        (
            "node:async_hooks.AsyncLocalStorage.bind",
            "Function",
            "Function.prototype",
        ),
        (
            "node:async_hooks.AsyncLocalStorage.snapshot",
            "Function",
            "Function.prototype",
        ),
        ("Object.fromEntries", "Object", "Object.prototype"),
        (
            "Object.getOwnPropertyDescriptor",
            "Object",
            "Object.prototype",
        ),
        (
            "%ArrayIteratorPrototype%.next",
            "Object",
            "Object.prototype",
        ),
        ("%GeneratorPrototype%.next", "Object", "Object.prototype"),
    ] {
        let row = index.last(owner).unwrap();
        assert!(row.births().any(|b| b == kind), "{owner}");
        let mut r = RealmReach::default();
        r.own(owner);
        r.close(&index);
        assert!(r.chain_roots.contains(proto), "{owner}: {r:?}");
        assert!(
            !r.names.contains(kind),
            "{owner} births an instance, not its constructor: {r:?}"
        );
    }
}

#[path = "realm_exec_tests.rs"]
mod realm_exec_tests;
