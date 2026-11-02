//! `global.gc()` (TypeScript's sys.ts `if (global.gc) global.gc();`): the
//! runtime's `gc` ignores `this`, so the call hands the global object to no
//! code. Only a write that may put a program (or interpreted) function under
//! a runtime global's key makes its call opaque.

use super::*;

const PRELUDE: &str = r#"
    class D { y = 2; }
    const d = new D();
    declare const k: string;
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
    let any = "(globalThis as any)[k] = function (this: any) { return this; };";
    assert!(d_escape(any, FactsConfig::default()).is_some());
}

#[test]
fn interpreted_code_may_write_every_global() {
    // A real `new Function` with a runtime source: interpreted code reads and
    // writes any global (E20), `gc` and `parseInt` included.
    assert!(d_escape("new Function(k)();", interp_cfg()).is_some());
}
