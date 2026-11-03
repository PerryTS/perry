use super::*;

fn lower(src: &str, name: &str) -> Module {
    let parsed = perry_parser::parse_typescript(src, name).expect("parse");
    perry_hir::lower_module(&parsed, name, name).expect("lower")
}
fn run(ms: &[(&str, &Module)]) -> Census {
    let modules: Vec<_> = ms.iter().map(|(p, m)| (p.to_string(), *m)).collect();
    Census::compute(
        &modules,
        &BTreeMap::new(),
        ms[0].0,
        false,
        &Default::default(),
    )
}
fn live(c: &Census, name: &str) -> bool {
    let ns: Vec<_> = c
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.label == name && n.symbol.is_some())
        .collect();
    assert!(!ns.is_empty(), "missing {name}: {:?}", c.nodes);
    ns.iter().any(|(n, _)| c.verdicts[*n].reachable)
}

#[test]
fn dead_functions_and_unused_exports_are_not_roots() {
    let m = lower("export function unusedExport() { dead(); } function dead() {} function alive() {} alive();", "entry.ts");
    let c = run(&[("entry.ts", &m)]);
    assert!(live(&c, "alive"));
    assert!(!live(&c, "dead"));
    assert!(!live(&c, "unusedExport"));
}

#[test]
fn dead_classes_do_not_seed_their_methods() {
    let m = lower(
        "class Dead { deadMethod() {} } class Live { liveMethod() {} } new Live().liveMethod();",
        "entry.ts",
    );
    let c = run(&[("entry.ts", &m)]);
    assert!(!live(&c, "deadMethod"));
    assert!(live(&c, "liveMethod"));
}

#[test]
fn callback_array_and_transitive_object_field_values_activate_bodies() {
    let m = lower("function callback() { leaf(); } function leaf() {} function dead() {} const a = [{ cb: callback }]; a[0].cb();", "entry.ts");
    let c = run(&[("entry.ts", &m)]);
    assert!(live(&c, "callback"));
    assert!(live(&c, "leaf"));
    assert!(!live(&c, "dead"));
}

#[test]
fn reachable_dynamic_import_observes_namespace_dead_site_does_not() {
    let mut m = lower("function neverCalled() {}", "entry.ts");
    m.init.push(Stmt::Expr(Expr::DynamicImport {
        paths: vec!["dynamic.ts".into()],
        arg: Box::new(Expr::String("dynamic.ts".into())),
        options: None,
        byte_offset: 0,
        deferred_error: None,
        synchronous: false,
    }));
    let mut dynamic = lower(
        "export function viaImport() {} function privateDead() {}",
        "dynamic.ts",
    );
    dynamic.init_kind = perry_hir::ModuleInitKind::Deferred;
    let c = run(&[("entry.ts", &m), ("dynamic.ts", &dynamic)]);
    assert!(live(&c, "viaImport"));
    assert!(!live(&c, "privateDead"));
    let import = m.init.pop().unwrap();
    m.functions[0].body.push(import);
    let c = run(&[("entry.ts", &m), ("dynamic.ts", &dynamic)]);
    assert!(!live(&c, "viaImport"));
}

#[test]
fn worker_entry_and_its_callback_are_roots_only_when_launched() {
    let mut m = lower("function dead() {}", "entry.ts");
    m.init.push(Stmt::Expr(Expr::WorkerNew {
        paths: vec!["worker.ts".into()],
        partial: false,
        filename: Box::new(Expr::String("worker.ts".into())),
        options: None,
        is_eval: false,
    }));
    let mut worker = lower(
        "function viaWorker() {} viaWorker(); function privateDead() {}",
        "worker.ts",
    );
    worker.init_kind = perry_hir::ModuleInitKind::Deferred;
    let c = run(&[("entry.ts", &m), ("worker.ts", &worker)]);
    assert!(live(&c, "viaWorker"));
    assert!(!live(&c, "privateDead"));
    m.init.pop();
    let c = run(&[("entry.ts", &m), ("worker.ts", &worker)]);
    assert!(!live(&c, "viaWorker"));
}

#[test]
fn direct_eval_keeps_enclosing_bindings_without_acquiring_other_private_scopes() {
    let m = lower("function viaEval() {} function hiddenOuter() { function hiddenInner() {} return 1; } eval(process.argv[2]);", "entry.ts");
    let c = run(&[("entry.ts", &m)]);
    assert!(live(&c, "viaEval"));
    // hiddenOuter is visible to module eval; its own scope is private until it
    // executes. Merely lowering a nested declaration is not a value escape.
    let n = lower("function privateFn() {} export const x = 1;", "other.ts");
    let c = run(&[("entry.ts", &m), ("other.ts", &n)]);
    assert!(!live(&c, "privateFn"));
}

#[test]
fn function_constructor_does_not_acquire_module_private_declarations() {
    let m = lower(
        "function privateFn() {} const f = Function(process.argv[2]); console.log(f());",
        "entry.ts",
    );
    let c = run(&[("entry.ts", &m)]);
    assert!(!live(&c, "privateFn"));
}

#[test]
fn class_effects_and_reflection_keep_observable_members() {
    let m = lower("class Effect { static x = console.log('effect'); method() {} } class Observed { method() {} } console.log(new Observed().method.name);", "entry.ts");
    let c = run(&[("entry.ts", &m)]);
    let effect = c
        .nodes
        .iter()
        .position(|n| n.label == "class:Effect")
        .unwrap();
    assert!(c.verdicts[effect].reachable);
    assert!(c
        .nodes
        .iter()
        .enumerate()
        .any(|(n, node)| node.label == "method" && c.verdicts[n].reachable));
}

#[test]
fn contract_rejects_a_generated_reference_to_a_dead_function() {
    let m = lower("function dead() {}", "entry.ts");
    let c = run(&[("entry.ts", &m)]);
    let symbol = c
        .nodes
        .iter()
        .find(|n| n.label == "dead")
        .unwrap()
        .symbol
        .clone()
        .unwrap();
    c.machines.lock().unwrap().push(MachineFunction {
        symbol: "main".into(),
        module: "entry.ts".into(),
        node: None,
        references: vec![symbol],
        precise_references: vec![],
    });
    assert!(c
        .check()
        .unwrap_err()
        .to_string()
        .contains("root contract violated"));
}

#[test]
fn generated_reference_walk_distinguishes_registration_and_executable_uses() {
    let lines = [
        "%ptr = ptrtoint ptr @dead to i64",
        "call void @js_register_class_constructor(i64 %ptr)",
    ]
    .map(String::from);
    assert!(!executable_references(&lines).contains(&"dead".to_string()));
    let mut lines = lines.to_vec();
    lines.push("store i64 %ptr, ptr @published".into());
    assert!(executable_references(&lines).contains(&"dead".to_string()));
    assert!(
        executable_references(&["%x = call double @dead()".into()]).contains(&"dead".to_string())
    );
}

#[test]
fn precision_accounting_does_not_call_conservative_only_bytes_dead() {
    let m = lower("class C { method() {} } new C();", "entry.ts");
    let c = run(&[("entry.ts", &m)]);
    let method = c.nodes.iter().position(|n| n.label == "method").unwrap();
    assert!(c.verdicts[method].reachable && c.verdicts[method].only_conservative);
}

#[test]
fn named_import_retains_only_the_observed_export_and_static_evaluation() {
    let entry = lower("import { called } from 'lib.ts'; called();", "entry.ts");
    let lib = lower(
        "export function called() {} export function unused() {} console.log('effect');",
        "lib.ts",
    );
    let c = run(&[("entry.ts", &entry), ("lib.ts", &lib)]);
    assert!(live(&c, "called"));
    assert!(!live(&c, "unused"));
    let n = c
        .nodes
        .iter()
        .position(|n| n.module == "lib.ts" && n.label == "module-evaluation")
        .unwrap();
    assert!(c.verdicts[n].reachable);
}

#[test]
fn interpreter_can_receive_a_late_store_without_a_private_scope_kill() {
    let m = lower("function lateCallback() {} function unreachableOuter() { function hidden() {} } const o: any = {}; (globalThis as any).published = o; o.cb = lateCallback; Function(process.argv[2])();", "entry.ts");
    let c = run(&[("entry.ts", &m)]);
    assert!(live(&c, "lateCallback"));
    assert!(!live(&c, "unreachableOuter"));
    let nested: Vec<_> = c
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.label.starts_with("closure#"))
        .collect();
    assert!(
        !nested.is_empty(),
        "lowering must retain the private nested body"
    );
    assert!(nested.iter().all(|(n, _)| !c.verdicts[*n].reachable));
}

#[test]
fn library_exports_are_entries_and_private_helpers_stay_private() {
    let m = lower(
        "export function publicEntry() {} function privateDead() {}",
        "entry.ts",
    );
    let modules = vec![("entry.ts".into(), &m)];
    let c = Census::compute(
        &modules,
        &BTreeMap::new(),
        "entry.ts",
        true,
        &Default::default(),
    );
    assert!(live(&c, "publicEntry"));
    assert!(!live(&c, "privateDead"));
}

#[test]
fn actual_generated_body_capture_rejects_missing_semantic_root() {
    let m = lower("function dead() {}", "entry.ts");
    let c = run(&[("entry.ts", &m)]);
    let symbol = c
        .nodes
        .iter()
        .find(|n| n.label == "dead")
        .unwrap()
        .symbol
        .clone()
        .unwrap();
    let mut llmod = crate::module::LlModule::new("x86_64-unknown-linux-gnu");
    let f = llmod.define_function("main", crate::types::I32, vec![]);
    let b = f.create_block("entry");
    b.call(crate::types::DOUBLE, &symbol, &[]);
    b.ret(crate::types::I32, "0");
    c.capture("entry.ts", &mut llmod).unwrap();
    assert!(c
        .check()
        .unwrap_err()
        .to_string()
        .contains("root contract violated"));
}

#[test]
fn named_star_import_does_not_observe_the_sibling_export() {
    let entry = lower("import { called } from 'barrel.ts'; called();", "entry.ts");
    let barrel = lower("export * from 'lib.ts';", "barrel.ts");
    let lib = lower(
        "export function called() {} export function unused() {}",
        "lib.ts",
    );
    let c = run(&[
        ("entry.ts", &entry),
        ("barrel.ts", &barrel),
        ("lib.ts", &lib),
    ]);
    assert!(live(&c, "called"));
    assert!(!live(&c, "unused"));
}

#[test]
fn dynamic_namespace_includes_star_bindings_and_deferred_evaluation() {
    let mut entry = lower("", "entry.ts");
    entry.init.push(Stmt::Expr(Expr::DynamicImport {
        paths: vec!["barrel.ts".into()],
        arg: Box::new(Expr::String("barrel.ts".into())),
        options: None,
        byte_offset: 0,
        deferred_error: None,
        synchronous: false,
    }));
    let mut barrel = lower("export * from 'lib.ts';", "barrel.ts");
    let mut lib = lower(
        "export function called() {} function sideEffect() {} sideEffect();",
        "lib.ts",
    );
    barrel.init_kind = perry_hir::ModuleInitKind::Deferred;
    lib.init_kind = perry_hir::ModuleInitKind::Deferred;
    let c = run(&[
        ("entry.ts", &entry),
        ("barrel.ts", &barrel),
        ("lib.ts", &lib),
    ]);
    assert!(live(&c, "called"));
    assert!(live(&c, "sideEffect"));
}

#[test]
fn duplicate_source_ids_union_lowered_bodies_and_symbols() {
    let mut m=lower("function one() { helperOne(); } function two() { helperTwo(); } function helperOne() {} function helperTwo() {} one();", "entry.ts");
    let id = m.functions.iter().find(|f| f.name == "one").unwrap().id;
    m.functions.iter_mut().find(|f| f.name == "two").unwrap().id = id;
    let c = run(&[("entry.ts", &m)]);
    assert!(live(&c, "helperOne"));
    assert!(live(&c, "helperTwo"));
    assert_eq!(
        c.node_for_symbol(&super::scoped_fn_name("entry_ts", "one")),
        c.node_for_symbol(&super::scoped_fn_name("entry_ts", "two"))
    );
}

#[test]
fn global_alias_reference_cannot_hide_a_missing_root() {
    let m = lower("function dead() {}", "entry.ts");
    let c = run(&[("entry.ts", &m)]);
    let symbol = c
        .nodes
        .iter()
        .find(|n| n.label == "dead")
        .unwrap()
        .symbol
        .clone()
        .unwrap();
    let mut ll = crate::module::LlModule::new("x86_64-unknown-linux-gnu");
    ll.add_raw_global(format!("@alias = constant ptr @{symbol}"));
    let b = ll
        .define_function("main", crate::types::I32, vec![])
        .create_block("entry");
    b.call_void("consume_callback", &[(crate::types::PTR, "@alias")]);
    b.ret(crate::types::I32, "0");
    c.capture("entry.ts", &ll).unwrap();
    assert!(c
        .check()
        .unwrap_err()
        .to_string()
        .contains("root contract violated"));
}

#[test]
fn metadata_registration_and_quoted_source_do_not_create_roots() {
    let m = lower("function dead() {}", "entry.ts");
    let c = run(&[("entry.ts", &m)]);
    let symbol = c
        .nodes
        .iter()
        .find(|n| n.label == "dead")
        .unwrap()
        .symbol
        .clone()
        .unwrap();
    let mut ll = crate::module::LlModule::new("x86_64-unknown-linux-gnu");
    ll.add_raw_global(format!("@inventory = constant ptr @{symbol}"));
    ll.add_raw_global(format!(
        "@source = constant [{} x i8] c\"@{symbol}\\00\"",
        symbol.len() + 2
    ));
    let b = ll
        .define_function("main", crate::types::I32, vec![])
        .create_block("entry");
    b.call_void(
        "js_register_class_declaration",
        &[(crate::types::PTR, "@inventory")],
    );
    b.call_void("consume_source", &[(crate::types::PTR, "@source")]);
    b.ret(crate::types::I32, "0");
    c.capture("entry.ts", &ll).unwrap();
    c.check().unwrap();
}

#[test]
fn generated_direct_call_refines_class_precision_without_changing_dead_set() {
    let m = lower("class C { method() {} } new C();", "entry.ts");
    let c = run(&[("entry.ts", &m)]);
    let n = c.nodes.iter().position(|n| n.label == "method").unwrap();
    assert!(c.verdicts[n].only_conservative);
    let sym = c.nodes[n].symbol.clone().unwrap();
    let mut ll = crate::module::LlModule::new("x86_64-unknown-linux-gnu");
    let b = ll
        .define_function("main", crate::types::I32, vec![])
        .create_block("entry");
    b.call(crate::types::DOUBLE, &sym, &[]);
    b.ret(crate::types::I32, "0");
    c.capture("entry.ts", &ll).unwrap();
    c.check().unwrap();
    assert!(c.precise_nodes()[n]);
    assert!(c.verdicts[n].reachable);
}

#[test]
fn computed_field_key_effect_is_a_root_without_instantiation() {
    let m = lower(
        "function keyEffect() { return 'k'; } class Unused { [keyEffect()] = 1; method() {} }",
        "entry.ts",
    );
    let c = run(&[("entry.ts", &m)]);
    assert!(live(&c, "keyEffect"));
}

#[test]
fn namespace_address_registration_is_not_observation_but_load_is() {
    let lines = [
        "%p = getelementptr i8, ptr @__perry_ns_lib_ts, i64 0",
        "call void @js_gc_register_global_root(ptr %p)",
        "store i64 0, ptr %p",
    ]
    .map(String::from);
    assert!(!executable_references(&lines)
        .iter()
        .any(|s| s.starts_with("__perry_namespace_observed_")));
    let mut lines = lines.to_vec();
    lines.push("%v = load double, ptr %p".into());
    assert!(executable_references(&lines).contains(&"__perry_namespace_observed_lib_ts".into()));
}

#[test]
fn independent_namespace_load_exposes_a_missing_namespace_root() {
    let entry = lower("import { used } from 'lib.ts'; used();", "entry.ts");
    let lib = lower(
        "export function used() {} export function unused() {}",
        "lib.ts",
    );
    let c = run(&[("entry.ts", &entry), ("lib.ts", &lib)]);
    let mut ll = crate::module::LlModule::new("x86_64-unknown-linux-gnu");
    let b = ll
        .define_function("main", crate::types::I32, vec![])
        .create_block("entry");
    b.emit_raw("%observed = load double, ptr @__perry_ns_lib_ts");
    b.ret(crate::types::I32, "0");
    c.capture("entry.ts", &ll).unwrap();
    assert!(c
        .check()
        .unwrap_err()
        .to_string()
        .contains("root contract violated"));
}

#[test]
fn renamed_global_getters_share_an_unobserved_binding_family() {
    let m = lower(
        "const a = class C { method() {} }; export { a as Public, a as Other }; console.log(a);",
        "entry.ts",
    );
    let c = run(&[("entry.ts", &m)]);
    let local = c.node_for_symbol("perry_fn_entry_ts__a").unwrap();
    assert_eq!(Some(local), c.node_for_symbol("perry_fn_entry_ts__Public"));
    assert_eq!(Some(local), c.node_for_symbol("perry_fn_entry_ts__Other"));
    assert!(!c.verdicts[local].reachable);
}

#[test]
fn renamed_function_implementation_keeps_its_body_without_rooting_export() {
    let m = lower(
        "function local() { return 1; } export { local as Public }; console.log(local());",
        "entry.ts",
    );
    let c = run(&[("entry.ts", &m)]);
    let local = c.node_for_symbol("perry_fn_entry_ts__local").unwrap();
    assert_eq!(Some(local), c.node_for_symbol("perry_fn_entry_ts__Public"));
    assert!(c.verdicts[local].reachable);
    let binding = c
        .nodes
        .iter()
        .position(|n| n.label == "export:Public")
        .unwrap();
    assert!(!c.verdicts[binding].reachable);
}

#[test]
fn class_alias_values_keep_the_underlying_class_family() {
    let mut m = lower("class Hidden { method() {} }", "entry.ts");
    m.classes[0].aliases.push("Alias".into());
    m.init.push(Stmt::Expr(Expr::ClassRef("Alias".into())));
    let c = run(&[("entry.ts", &m)]);
    assert!(live(&c, "method"));
}

#[test]
fn actual_tiny_entry_image_preserves_the_generated_reference_contract() {
    let m = lower("function dead() {}", "entry.ts");
    let c = run(&[("entry.ts", &m)]);
    let symbol = c
        .nodes
        .iter()
        .find(|n| n.label == "dead")
        .unwrap()
        .symbol
        .as_ref()
        .unwrap();
    let ir=format!("declare double @{symbol}()\ndefine i32 @main() {{\n %r = call double @{symbol}()\n ret i32 0\n}}\n");
    c.capture_tiny_ir(&ir).unwrap();
    assert!(c
        .check()
        .unwrap_err()
        .to_string()
        .contains("root contract violated"));
}

#[test]
fn well_known_symbol_class_hooks_are_owned_by_module_init() {
    let m = lower(
        "class Dead { get [Symbol.toStringTag]() { return 'D'; } } function dead() {}",
        "entry.ts",
    );
    let c = run(&[("entry.ts", &m)]);
    assert!(live(&c, "__perry_wk_tostringtag_Dead"));
    assert!(!live(&c, "dead"));
}
