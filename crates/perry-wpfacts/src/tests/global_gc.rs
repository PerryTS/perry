//! `global.gc()` (TypeScript's sys.ts `if (global.gc) global.gc();`): the
//! runtime's `gc` ignores `this`, so the call hands the global object to no
//! code. Only a write that may put a program (or interpreted) function under
//! a runtime global's key makes its call opaque.

use super::*;

const PRELUDE: &str = r#"
    class D { y = 2; }
    const d = new D();
    const k: string = process.argv[2];
"#;

const GC: &str = "if ((globalThis as any).gc) { (globalThis as any).gc(); }";
/// The runtime's `parseInt` reads `d` through ToPrimitive and keeps nothing;
/// a written `parseInt` is an opaque call that takes `d`.
const USE: &str = "(globalThis as any).parseInt(d);\nconsole.log(d.y);";

fn d_escape(body: &str, cfg: FactsConfig) -> Option<String> {
    let pf = facts_with(&format!("{PRELUDE}{body}\n{GC}\n{USE}"), cfg);
    tmpl(&pf, "D").escape.clone()
}

#[test]
fn runtime_gc_keeps_the_global_object_known() {
    assert_eq!(d_escape("", FactsConfig::default()), None);
}

#[test]
fn a_computed_global_write_may_be_any_runtime_global() {
    // `k` may name `parseInt`, so the call may call the written function,
    // which ignores its argument: `d` stays known (the argument-handing
    // and the ignoring cases below show the call is the written one).
    let any = "(globalThis as any)[k] = function (this: any) { return this; };";
    assert_eq!(d_escape(any, FactsConfig::default()), None);
}

#[test]
fn interpreted_code_may_write_every_global() {
    // A real `new Function` with a runtime source: interpreted code reads and
    // writes any global (E20), `gc` and `parseInt` included.
    assert!(d_escape("new Function(k)();", interp_cfg()).is_some());
}

#[test]
fn a_computed_global_write_of_a_function_that_hands_on_its_argument() {
    // `k` may name `parseInt`: `globalThis.parseInt(d)` may call the
    // written function, which hands `d` to unknown code.
    let any = "declare function external(x?: unknown): any;\n(globalThis as any)[k] = function (x: any) { return external(x); };";
    assert!(d_escape(any, FactsConfig::default()).is_some());
}

#[test]
fn a_named_global_write_of_a_function_that_hands_on_its_argument() {
    let named = "declare function external(x?: unknown): any;\n(globalThis as any).parseInt = function (x: any) { return external(x); };";
    assert!(d_escape(named, FactsConfig::default()).is_some());
}

#[test]
fn a_computed_global_write_of_a_function_that_ignores_its_argument() {
    // Control: the written function keeps nothing of `d`.
    let any = "declare function external(x?: unknown): any;\n(globalThis as any)[k] = function (x: any) { return external(); };";
    assert_eq!(d_escape(any, FactsConfig::default()), None);
}
