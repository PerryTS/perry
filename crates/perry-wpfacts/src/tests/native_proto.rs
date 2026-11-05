//! Native parent prototypes are runtime-owned: a key one does not define
//! continues the chain, unless the program (or interpreted code) may write
//! that native prototype under the key.

use super::*;

const PRELUDE: &str = r#"
    import { EventEmitter } from "events";
    class D { y = 2; }
    class C extends EventEmitter { constructor() { super(); } }
    const c: any = new C();
    const d = new D();
    declare const k: string;
"#;

fn d_escape(body: &str, cfg: FactsConfig) -> Option<String> {
    let pf = facts_with(&format!("{PRELUDE}{body}\nconsole.log(d.y);"), cfg);
    tmpl(&pf, "D").escape.clone()
}

#[test]
fn missing_native_key_continues_the_chain() {
    assert_eq!(d_escape("c.absent?.(d);", FactsConfig::default()), None);
    assert_eq!(
        d_escape("const g = c.absent; if (g) g(d);", FactsConfig::default()),
        None
    );
}

#[test]
fn defined_native_key_stays_opaque() {
    // `setMaxListeners` is EventEmitter's own: an opaque builtin value.
    let g = "const g = c.setMaxListeners; g(d);";
    assert!(d_escape(g, FactsConfig::default()).is_some());
}

#[test]
fn written_native_prototype_keeps_the_escape() {
    let w = "EventEmitter.prototype.absent = function (o: any) { (globalThis as any).kept = o; };";
    assert!(d_escape(&format!("{w} c.absent?.(d);"), FactsConfig::default()).is_some());
    // Written after the lookup was solved: replayed.
    assert!(d_escape(&format!("c.absent?.(d); {w}"), FactsConfig::default()).is_some());
    // Another key: this lookup still passes.
    let other = "EventEmitter.prototype.other = function (o: any) { return o; };";
    assert_eq!(
        d_escape(&format!("{other} c.absent?.(d);"), FactsConfig::default()),
        None
    );
    // A computed key may be any key.
    let any = "(EventEmitter.prototype as any)[k] = function (o: any) { return o; };";
    assert!(d_escape(&format!("{any} c.absent?.(d);"), FactsConfig::default()).is_some());
    let del = "delete (EventEmitter.prototype as any).absent;";
    assert!(d_escape(&format!("{del} c.absent?.(d);"), FactsConfig::default()).is_some());
}

#[test]
fn interpreted_code_is_the_guard_not_a_kill() {
    // Decision 150: interpreted code that may write a native prototype is
    // the runtime's PERRY_INTERP_PROTO_WRITTEN guard on every proof, not a
    // static escape of everything that passes one.
    let src = format!("{PRELUDE}c.absent?.(d); new Function(k)();\nconsole.log(d.y);");
    let pf = facts_with(&src, interp_cfg());
    assert!(pf.interp_guard.is_some());
    assert_eq!(tmpl(&pf, "D").escape, None);
}
