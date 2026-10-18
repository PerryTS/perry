use super::{facts, facts_with};
use crate::FactsConfig;

#[test]
fn unused_bodies_do_not_reach_loader_capabilities() {
    for s in [
        "function unused() { return process.getBuiltinModule('module'); } console.log(1);",
        "const unused = () => globalThis.process.getBuiltinModule('module'); console.log(1);",
        "class Unused { m() { return process.getBuiltinModule('module'); } } console.log(1);",
        "function unused(src: string) { return new Function(src); } console.log(1);",
    ] {
        let p = facts(s);
        assert!(p.module_reentry.is_none(), "{s}: {:?}", p.module_reentry);
    }
}

#[test]
fn naming_global_objects_and_safe_members_is_not_loader_reach() {
    for s in [
        "const root = globalThis; console.log(typeof root);",
        "const root = globalThis; console.log(typeof root.Symbol);",
        "console.log(process.argv.length);",
        "const own = {cache: 1, createRequire: 2, constructor: 3}; console.log(own.cache, own.createRequire, own.constructor);",
    ] {
        let p=facts(s);
        assert!(p.module_reentry.is_none(), "{s}: {:?}",p.module_reentry);
    }
}

#[test]
fn reachable_loader_reads_and_indirect_calls_refuse_run_once() {
    for s in [
        "process.getBuiltinModule('module');",
        "function compile(src: string) { return Function(src); } compile(process.argv[2]);",
        "const key = Symbol(); const own = {get [key]() { return process.getBuiltinModule('module'); }}; console.log(own[key]);",
        "class Used { get [Symbol.iterator]() { return process.getBuiltinModule('module'); } } const own = new Used(); console.log(own[Symbol.iterator]);",
        "const loader = () => process.getBuiltinModule('module'); loader();",
        "function loader() { return process.getBuiltinModule('module'); } const alias = loader; alias();",
        "const root = globalThis; root.process.getBuiltinModule('module');",
        "function compile(src: string) { return new Function(src); } compile(process.argv[2]);",
        "const f = () => 0; const key = ['con', 'structor'].join(''); const dynamic = f[key]; dynamic(process.argv[2]);",
    ] {
        let p=facts(s);
        assert!(p.module_reentry.is_some(),"{s}: {}",p.dump_text());
    }
}

#[test]
fn callbacks_accessors_and_constructed_bodies_are_reachable() {
    for s in [
        "const loader = () => process.getBuiltinModule('module'); [1].map(loader);",
        "const loader = () => process.getBuiltinModule('module'); setTimeout(loader, 0);",
        "const own = {get loader() { return process.getBuiltinModule('module'); }}; console.log(own.loader);",
        "class Used { constructor() { process.getBuiltinModule('module'); } } new Used();",
        "class Used { x = process.getBuiltinModule('module'); } new Used();",
    ] {
        let p=facts(s);
        assert!(p.module_reentry.is_some(),"{s}: {}",p.dump_text());
    }
}

#[test]
fn aborted_solve_cannot_prove_module_run_once() {
    let p = facts_with(
        "class P { x=1; } new P();",
        FactsConfig {
            max_work: Some(1),
            ..Default::default()
        },
    );
    assert!(p.stats.aborted);
    assert!(p.module_reentry.is_some());
}

#[test]
fn bound_unused_body_is_not_invoked() {
    let p=facts("const loader=()=>process.getBuiltinModule('module'); const unused=loader.bind(null); console.log(typeof unused);");
    assert!(p.module_reentry.is_none(), "{}", p.dump_text());
}
#[test]
fn widened_calls_reach_escaped_user_bodies() {
    let p=facts_with("const fs=[()=>1,()=>2,()=>3,()=>4,()=>process.getBuiltinModule('module')]; function choose(i:number){return fs[i];} choose(4)();",FactsConfig {widen_k:Some(2),..Default::default()});
    assert!(p.module_reentry.is_some(), "{}", p.dump_text());
}

#[test]
fn reflection_and_aliased_builtins_keep_capability_edges() {
    for s in [
        "const get=Object.getOwnPropertyDescriptor; const d=get(globalThis,'Function'); d.value(process.argv[2]);",
        "Reflect.get(globalThis,'Function')(process.argv[2]);",
        "const own={get x(){return process.getBuiltinModule('module');}}; const get=Object.getOwnPropertyDescriptor; get(own,'x').get();",
    ] {let p=facts(s);assert!(p.module_reentry.is_some(),"{s}: {}",p.dump_text());}
}
