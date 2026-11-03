use super::summary::{summarize, Eval, Kind};

fn parse(src: &str) -> swc_ecma_ast::Module {
    perry_parser::parse_typescript(src, "fixture.ts").expect("parses")
}

fn inert(src: &str) -> Vec<bool> {
    summarize(&parse(src), false)
        .items
        .iter()
        .map(|i| matches!(i.eval, Eval::Inert(_)))
        .collect()
}

#[test]
fn declarations_literals_and_inert_classes_create_bindings_only() {
    for src in [
        "export function f(x = effect()) { return effect(x); }",
        "export const a = 1, b = 'x', c = [1, { k: null }], d = () => effect();",
        "export class C { m() { effect(); } static s = 1; x = effect(); get g() { return 1; } }",
        "function base() {} export class D extends base {}",
        "class A {} export class B extends A { static #p = 2; }",
        "export const o = { m() { return effect(); }, get g() { return 1; } };",
        "export enum E { A = 1, B = 'b', C }",
        "export default function () { throw new Error(); }",
        "export const t = typeof undefined;",
        "export interface I { x: number }",
    ] {
        let s = inert(src);
        assert!(s.iter().all(|x| *x), "{src}: {s:?}");
    }
}

#[test]
fn anything_that_can_run_code_or_throw_is_kept() {
    for (src, at) in [
        ("export const a = effect();", 0),
        ("export const a = missing;", 0),
        ("export const a = o.getter;", 0),
        ("export const { a } = o;", 0),
        ("export const a = [...xs];", 0),
        ("export const a = { ...o };", 0),
        ("export const a = { [k]: 1 };", 0),
        ("export class C { static { effect(); } }", 0),
        ("export class C { static s = effect(); }", 0),
        ("export class C extends effect() {}", 0),
        ("export class C extends Later {} class Later {}", 0),
        ("export class C { [k]() {} }", 0),
        ("@dec export class C {}", 0),
        ("export const a = `x${y}`;", 0),
        ("export enum E { A = f() }", 0),
        ("console.log(1);", 0),
        ("const b = a; const a = 1;", 0),
    ] {
        let s = inert(src);
        assert!(!s[at], "{src}: {s:?}");
    }
}

#[test]
fn imported_reads_wait_for_the_graph() {
    let s = summarize(
        &parse("import { B } from './b'; export class C extends B {} export const x = B;"),
        false,
    );
    match &s.items[1].eval {
        Eval::Inert(conds) => assert_eq!(conds.len(), 1),
        other => panic!("{other:?}"),
    }
    match &s.items[2].eval {
        Eval::Inert(conds) => assert_eq!(conds.len(), 1),
        other => panic!("{other:?}"),
    }
}

#[test]
fn mentions_follow_names_and_namespace_members() {
    let s = summarize(
        &parse("import * as ns from './n'; import { a } from './a'; export const f = () => ns.x + a; export const g = () => ns;"),
        false,
    );
    assert_eq!(s.items[0].kind, Kind::Import);
    assert!(s.items[0].mentions.is_empty());
    assert!(s.items[2].mentions.contains("a"));
    assert!(s.items[2]
        .members
        .contains(&("ns".to_string(), "x".to_string())));
    assert!(!s.items[2].escapes.contains("ns"));
    assert!(s.items[3].escapes.contains("ns"));
}

#[test]
fn eval_and_with_keep_the_module_whole() {
    assert!(!summarize(&parse("export const a = 1; eval('a');"), false).prunable);
    assert!(summarize(&parse("export const a = 1; (0, eval)('a');"), false).prunable);
    assert!(!summarize(&parse("export const a = 1;"), true).prunable);
}

#[test]
fn prune_keeps_every_edge() {
    let module = parse(
        "import { a, b } from './a'; export { c } from './c'; export { d as e } from './d'; export const x = a; export const y = 2;",
    );
    let s = summarize(&module, false);
    let plan = super::Prune {
        fingerprint: s.fingerprint,
        items: [4].into_iter().collect(),
        imports: ["b".to_string()].into_iter().collect(),
        exports: ["c".to_string()].into_iter().collect(),
    };
    let out = super::prune::prune(&module, &plan);
    assert_eq!(out.body.len(), 4);
    let text = format!("{:?}", out.body);
    assert!(!text.contains("\"y\""), "{text}");
    assert!(text.contains("\"./c\""), "the edge stays: {text}");
}
