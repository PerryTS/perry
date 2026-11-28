//! Rule W10-IDX: an Array.prototype method run on a `this` that is no
//! Array (ECMA-262 23.1.3: Set(O, ! ToString(𝔽(k)), v, true) and
//! Set(O, "length", ...)) writes only index keys and `length`, never a
//! name that is no index. c09 (WIDEN10): `push`/`unshift` applied to
//! `globalThis` booked their arguments under any String key of the global
//! object, so every read of a named global (`Object`, `Math`, ...) held
//! them. A computed-key store with an unknown String key still names any
//! global (the control).
use super::*;

fn src(body: &str, read: &str) -> String {
    format!(
        "class Box {{ v: any; constructor(v: any) {{ this.v = v; }} }}
         class Ev {{ e = 1; }}
         const i: number = process.argv.length > 9 ? 2 : 0;
         const k: string = process.argv[2];
         const push: any = Array.prototype.push;
         const unshift: any = Array.prototype.unshift;
         const splice: any = Array.prototype.splice;
         const g: any = globalThis;
         const o: any = {{ name: 1 }};
         {body}
         const b = new Box({read});
         console.log(b.v);"
    )
}

/// Whether `b.v` may hold an `Ev`, or is ⊤.
fn ev(s: &str) -> bool {
    let pf = facts(s);
    let k = &field(tmpl(&pf, "Box"), "v").kind;
    k.top || k.objs.iter().any(|o| o.contains("Ev"))
}

#[test]
fn push_on_global_skips_named_global() {
    assert!(!ev(&src("push.call(globalThis, new Ev());", "g.Object")));
    assert!(!ev(&src("push.call(g, new Ev());", "g.someName")));
}

#[test]
fn push_on_global_reaches_index() {
    assert!(ev(&src("push.call(g, new Ev());", "g[i]")));
}

#[test]
fn number_key_store_on_global_skips_named_global() {
    assert!(!ev(&src("g[0] = new Ev();", "g.someName")));
    assert!(!ev(&src("g[i] = new Ev();", "g.Object")));
    assert!(ev(&src("g[i] = new Ev();", "g[i]")));
}

#[test]
fn computed_store_on_global_reaches_named_global() {
    assert!(ev(&src("g[k] = new Ev();", "g.Object")));
}

#[test]
fn array_methods_on_object_skip_named_field() {
    for body in [
        "push.call(o, new Ev());",
        "unshift.call(o, new Ev());",
        "splice.call(o, 0, 0, new Ev());",
        "Array.prototype.fill.call(o, new Ev());",
    ] {
        assert!(!ev(&src(body, "o.name")), "{body}");
        assert!(ev(&src(body, "o[i]")), "{body}");
    }
}

#[test]
fn array_method_on_object_reads_numeric_fields() {
    // `forEach` on `{ 0: ev, length: 1 }` hands index 0 to the callback.
    assert!(ev(&src(
        "let got: any; const a: any = { 0: new Ev(), length: 1 };
         Array.prototype.forEach.call(a, (x: any) => { got = x; });",
        "got"
    )));
}


