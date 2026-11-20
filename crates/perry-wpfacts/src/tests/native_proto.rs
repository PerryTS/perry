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
    const k: string = process.argv[2];
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

/// A key written onto the native prototype holds what the write stored
/// (ECMA-262 10.1.9.2): the lookup runs that function with the argument,
/// at once or replayed when the write comes later. Its argument escapes
/// where that function hands it to unknown code, or where the stored value
/// itself comes from unknown code; a function that only returns it, or a
/// delete (the key is absent: the optional call does not run), lets nothing
/// escape.
#[test]
fn written_native_prototype_keeps_the_escape() {
    let cfg = FactsConfig::default;
    // Unknown code: a module outside the program, run at load.
    let unknown = "import \"unseen-host\"; const unk: any = (globalThis as any).unseenFn;";
    let w = "EventEmitter.prototype.absent = function (o: any) { unk(o); };";
    assert!(d_escape(&format!("{unknown} {w} c.absent?.(d);"), cfg()).is_some());
    // Written after the lookup was solved: replayed.
    assert!(d_escape(&format!("{unknown} c.absent?.(d); {w}"), cfg()).is_some());
    // The stored value comes from unknown code.
    let opaque = "EventEmitter.prototype.absent = unk;";
    assert!(d_escape(&format!("{unknown} {opaque} c.absent?.(d);"), cfg()).is_some());
    // A stored function that keeps the argument nowhere unknown code sees.
    let kept =
        "EventEmitter.prototype.absent = function (o: any) { (globalThis as any).kept = o; };";
    assert_eq!(d_escape(&format!("{kept} c.absent?.(d);"), cfg()), None);
    // Another key: this lookup still passes.
    let other = "EventEmitter.prototype.other = function (o: any) { return o; };";
    assert_eq!(d_escape(&format!("{other} c.absent?.(d);"), cfg()), None);
    // A computed key may be any key: the stored function runs.
    let any = "(EventEmitter.prototype as any)[k] = function (o: any) { unk(o); };";
    assert!(d_escape(&format!("{unknown} {any} c.absent?.(d);"), cfg()).is_some());
    let ret = "(EventEmitter.prototype as any)[k] = function (o: any) { return o; };";
    assert_eq!(d_escape(&format!("{ret} c.absent?.(d);"), cfg()), None);
    let del = "delete (EventEmitter.prototype as any).absent;";
    assert_eq!(d_escape(&format!("{del} c.absent?.(d);"), cfg()), None);
}

#[test]
fn interpreted_code_is_the_guard_not_a_kill() {
    // Decision 150: interpreted code that may write a native prototype is
    // the runtime's PERRY_INTERP_PROTO_WRITTEN guard on every proof, not a
    // static escape of everything that passes one. The receiver is built by a
    // native constructor directly: its prototype is not a replaceable parent
    // binding, so the only thing interpreted code can do is write the native
    // prototype, which the guard covers.
    let src = r#"
        class D { y = 2; }
        const m: any = new Error("x");
        const d = new D();
        const k: string = process.argv[2];
        m.absent?.(d); new Function(k)();
        console.log(d.y);
    "#;
    let pf = facts_with(src, interp_cfg());
    assert!(pf.interp_guard.is_some());
    assert_eq!(tmpl(&pf, "D").escape, None);
}

#[test]
fn interpreted_code_replaces_a_named_import_parent() {
    // Since 5be5c92e4e interpreted code writes globals and module exports for
    // real, and super() runs the parent value. Node allows
    // syncBuiltinESMExports to replace the `events` export, so a subclass of
    // the named import is constructed by a value interpreted code may supply:
    // it escapes through its parent (top super), not through a native
    // prototype write.
    let src = format!("{PRELUDE}c.absent?.(d); new Function(k)();\nconsole.log(d.y);");
    let pf = facts_with(&src, interp_cfg());
    assert!(pf.interp_guard.is_some());
    assert!(tmpl(&pf, "C").escape.is_some());
}
