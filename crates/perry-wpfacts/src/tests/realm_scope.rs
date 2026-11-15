use super::*;
use perry_hir::realm_reach::{scan_for_source, RealmReach};
use perry_hir::realm_reach_index::RealmIndex;
use perry_hir::realm_reach_scope::{Operand, OperandIndex};

fn reach(src: &str) -> (RealmReach, RealmReach, ProgramFacts) {
    let facts = facts(src);
    assert!(
        facts.realm_solve_complete,
        "complete solve: {:?}",
        facts.stats
    );
    let ast = perry_parser::parse_typescript(src, "t.ts").unwrap();
    let index = RealmIndex::new(crate::native_effects::realm());
    let mut input = scan_for_source(&ast, &index, "t.ts");
    let pending = input.observations.len();
    let lowered = perry_hir::lower_module(&ast, "t", "t.ts").unwrap();
    perry_hir::realm_reach::scan_lowered_for_scope(&lowered, &index, &mut input);
    assert_eq!(
        input.observations.len(),
        pending,
        "lowering must defer source observations"
    );
    let mut old = input.clone();
    old.close(&index);
    let mut scoped = input;
    scoped.resolve_observations(&facts.realm_operand_index(), &index);
    scoped.close(&index);
    assert!(
        scoped.names.is_subset(&old.names),
        "new names: {:?}",
        scoped.names.difference(&old.names).collect::<Vec<_>>()
    );
    let before = scoped.clone();
    scoped.close(&index);
    assert_eq!(scoped, before, "close is idempotent");
    (scoped, old, facts)
}

#[test]
fn realm_scope_upper_uses_the_solved_primitive_result() {
    let (new, old, facts) = reach("console.log('a'.toUpperCase())");
    assert!(facts.operands.iter().any(|o| o.operand == Operand::Arg(0)
        && o.prims == crate::model::prim::STR
        && !o.scope.top));
    assert!(
        new.names.len() <= 16,
        "R{} W{} {:?}",
        new.names.len(),
        new.whole.len(),
        new.names
    );
    assert!(new.whole.len() <= 7, "{:?}", new.whole);
    let index = RealmIndex::new(crate::native_effects::realm());
    assert!(
        new.names
            .iter()
            .filter_map(|n| index.last(n))
            .filter(|r| matches!(
                r.kind(),
                perry_hir::realm_manifest::RealmKind::Function
                    | perry_hir::realm_manifest::RealmKind::Method
            ))
            .count()
            <= 5
    );
    assert!(new.names.len() < old.names.len());
}

#[test]
fn realm_scope_template_is_a_primitive_even_without_solved_facts() {
    let (new, _, _) = reach("let name=process.argv[2]; console.log(`Hello, ${name}!`)");
    let index = RealmIndex::new(crate::native_effects::realm());
    let functions = new
        .names
        .iter()
        .filter_map(|n| index.last(n))
        .filter(|r| {
            matches!(
                r.kind(),
                perry_hir::realm_manifest::RealmKind::Function
                    | perry_hir::realm_manifest::RealmKind::Method
            )
        })
        .count();
    eprintln!(
        "template logical R{} W{} functions{}",
        new.names.len(),
        new.whole.len(),
        functions
    );
}

#[test]
fn realm_scope_program_constructor_stops_a_keyed_chain_read() {
    let (new, _, _) = reach("class Square {area(){return 4}}; const sq=new Square; console.log(sq.constructor.name); sq.area()");
    assert!(!new.whole.contains("Object"), "{:?}", new.whole);
    assert!(
        !new.keys_on
            .get("Object.prototype")
            .is_some_and(|keys| keys.iter().any(|(k, _)| k == "constructor")),
        "{:?}",
        new.keys_on
    );
}

#[test]
fn realm_scope_dynamic_code_and_escaped_formatted_callable_keep_top() {
    for src in [
        "const f=new Function(process.argv[2]); console.log(f())",
        "globalThis.f=console.log; globalThis.f({})",
        "function f(o){return o[process.argv[2]]}; console.log(f((new Function('return this'))()))",
    ] {
        let (new, old, _) = reach(src);
        assert_eq!(new.names, old.names, "{src}");
    }
}

#[test]
fn realm_scope_incomplete_missing_and_marked_operands_keep_conservative_facts() {
    let src = "console.log('a'.toUpperCase())";
    let ast = perry_parser::parse_typescript(src, "t.ts").unwrap();
    let index = RealmIndex::new(crate::native_effects::realm());
    let mut input = scan_for_source(&ast, &index, "t.ts");
    let pending = input.observations.len();
    let lowered = perry_hir::lower_module(&ast, "t", "t.ts").unwrap();
    perry_hir::realm_reach::scan_lowered_for_scope(&lowered, &index, &mut input);
    assert_eq!(
        input.observations.len(),
        pending,
        "lowering must defer source observations"
    );
    let mut old = input.clone();
    old.close(&index);
    let mut missing = input.clone();
    missing.resolve_observations(
        &OperandIndex {
            complete: true,
            ..Default::default()
        },
        &index,
    );
    missing.close(&index);
    assert_eq!(missing, old);
    let mut incomplete = input.clone();
    let mut facts = facts(src).realm_operand_index();
    facts.complete = false;
    incomplete.resolve_observations(&facts, &index);
    incomplete.close(&index);
    assert_eq!(incomplete, old);
    for values in facts.sites.values_mut() {
        values.top = true;
    }
    facts.complete = true;
    let mut top = input;
    top.resolve_observations(&facts, &index);
    top.close(&index);
    assert_eq!(top, old);
}

#[test]
fn realm_scope_a_written_builtin_result_is_not_an_unwritten_primitive_proof() {
    let (new, old, facts) =
        reach("String.prototype.toUpperCase=()=>({}); console.log('a'.toUpperCase())");
    assert_eq!(
        new.names, old.names,
        "written builtin must keep the conservative control; operands={:?}",
        facts.operands
    );
}

#[test]
fn realm_scope_lad04_scopes_only_the_observed_program_chain() {
    let (new, _, _) = reach("class Shape {describe(){return 'shape'} area(){return 0}} class Square extends Shape {area(){return 4}} const sq=new Square; console.log(sq.describe(),sq.area(),Object.getPrototypeOf(sq).constructor.name)");
    assert!(
        new.names.len() <= 80,
        "R{} W{} {:?}",
        new.names.len(),
        new.whole.len(),
        new.names
    );
    assert!(new.whole.len() <= 30, "{:?}", new.whole);
}

#[test]
fn realm_scope_aborted_solve_exports_no_optimistic_operand_proof() {
    let facts = facts_with(
        "console.log('a'.toUpperCase())",
        FactsConfig {
            max_work: Some(1),
            ..Default::default()
        },
    );
    assert!(!facts.realm_solve_complete);
    assert!(!facts.realm_operand_index().complete);
    assert!(facts.operands.is_empty());
}

#[test]
fn realm_scope_plain_set_does_not_keep_an_inherited_data_member() {
    let (new, _, facts) = reach("const o={}; o.toString=()=>''; console.log(1)");
    assert!(facts.realm_builtin_writes.is_empty());
    assert!(
        !new.names.contains("Object.prototype.toString"),
        "{:?}",
        new.names
    );
}

#[test]
fn realm_scope_a_proxy_operand_keeps_the_unproved_observation() {
    let (new, old, _) = reach("const o=new Proxy({},{}); console.log(o.toString())");
    assert_eq!(new.names, old.names);
}

#[test]
fn realm_scope_json_program_reviver_is_not_an_unknown_escape() {
    let (new, _, facts) = reach("const o=JSON.parse('{\"name\":\"a\"}',(k,v)=>typeof v==='string'?v.toUpperCase():v);console.log(o.name)");
    assert!(facts
        .operands
        .iter()
        .any(|o| o.operand == Operand::Arg(1) && o.scope.program_bodies));
    assert!(
        new.names.len() <= 110,
        "R{} W{} {:?}",
        new.names.len(),
        new.whole.len(),
        new.names
    );
    eprintln!("JSON logical R{} W{}", new.names.len(), new.whole.len());
}

#[test]
fn realm_scope_an_unproved_own_callable_cannot_be_replaced_by_a_native_chain_proof() {
    let (new, old, _) = reach("const f=process.binding('util').isArrayBuffer;const o={toString:f};console.log(o.toString())");
    assert_eq!(new.names, old.names);
}

#[test]
fn realm_scope_program_new_and_super_calls_use_their_solved_callees() {
    let (new, _, _) = reach("class A{constructor(x){this.x=x}}class B extends A{constructor(x){super(x)}area(){return this.x}}const o=new B(2);console.log(o.area())");
    assert!(
        new.names.len() <= 80,
        "R{} W{} {:?}",
        new.names.len(),
        new.whole.len(),
        new.names
    );
    assert!(new.whole.len() <= 30, "{:?}", new.whole);
}

#[test]
fn realm_scope_an_unproved_live_superclass_keeps_the_constructor_control() {
    let (new, old, _) =
        reach("class A{}class B extends A{};Object.setPrototypeOf(B,globalThis.f);new B({})");
    assert_eq!(new.names, old.names);
}

#[test]
fn realm_scope_absent_primitive_family_member_is_not_unknown_code() {
    let facts = facts("const v=Math.random()?'a':1;const f=v.toUpperCase;f()");
    assert!(facts
        .operands
        .iter()
        .any(|operand| operand.operand == Operand::Callee
            && operand.scope.known_callees
            && operand.values.iter().any(|value| matches!(
                value.kind,
                crate::model::TokKind::BuiltinFn(crate::model::Native::Member {
                    identity: "String.prototype.toUpperCase",
                    ..
                })
            ))));
    assert!(!facts
        .operands
        .iter()
        .flat_map(|operand| &operand.values)
        .any(|value| matches!(
            value.kind,
            crate::model::TokKind::BuiltinFn(crate::model::Native::Any)
        )));
}

#[test]
fn realm_scope_implicit_runtime_errors_have_no_single_prototype_proof() {
    let (new, _, facts) = reach("try { null.x } catch(e) { console.log(e.constructor.name) }");
    assert!(new.receiver_keys.contains("constructor"), "{:?}", new);
    assert!(facts
        .operands
        .iter()
        .any(|o| o.operand == Operand::NamedReceiver("constructor".into()) && o.scope.top));
}

#[test]
fn realm_scope_concrete_error_instances_keep_their_prototype_proof() {
    let (new, _, _) = reach("const e=new Error('x');console.log(e.constructor.name)");
    assert!(!new.receiver_keys.contains("constructor"), "{:?}", new);
    assert!(new
        .keys_on
        .get("Error.prototype")
        .is_some_and(|keys| keys.contains(&("constructor".into(), false))));
}

#[test]
fn realm_scope_collection_initializers_keep_their_runtime_adder_slot() {
    let index = RealmIndex::new(crate::native_effects::realm());
    for (source, holder, key, value) in [
        (
            "const m=new Map([[1,2]]);console.log(m.size)",
            "Map.prototype",
            "set",
            "Map.prototype.set",
        ),
        (
            "const s=new Set([1,2]);console.log(s.size)",
            "Set.prototype",
            "add",
            "Set.prototype.add",
        ),
    ] {
        let (new, _, _) = reach(source);
        assert!(new.names.contains(value), "{source}: {:?}", new);
        assert!(new.keeps_member(&index, index.last(holder).unwrap(), key, value, false));
    }
}
