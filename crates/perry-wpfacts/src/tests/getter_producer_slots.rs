//! Realm getters that read an internal slot (`returns(islot(this, S))`) are
//! exact only when every declared birth gives the slot and every declared
//! writer stores it: the abort event names its signal as `target`
//! (event_target/abort.rs dispatches on the signal), a CustomEvent keeps
//! `detail` internal (null when absent), and a view `.buffer` is the
//! buffer the view was made on ([[ViewedArrayBuffer]]).
use super::*;
use crate::model::prim;

fn stored(prelude: &str, read: &str) -> crate::derive::KindFact {
    let pf = facts(&format!(
        "class H {{ v: any = 0; }}\nclass D {{ q = 1; }}\n{prelude}\nconst h = new H();\nh.v = {read};\nconsole.log(h.v);\n"
    ));
    field(tmpl(&pf, "H"), "v").kind.clone()
}

#[test]
fn abort_event_target_is_the_signal() {
    let k = stored(
        "const c = new AbortController(); const g: any = { v: 0 }; c.signal.onabort = (e: any) => { g.v = e.target; }; c.abort();",
        "g.v",
    );
    assert!(!k.top && k.builtin_objs > 0, "{k:?}");
}

#[test]
fn custom_event_detail_is_internal_and_null_when_absent() {
    let k = stored("const e = new CustomEvent(\"x\");", "e.detail");
    assert!(!k.top && k.prims & prim::NULL != 0, "{k:?}");
    let k = stored(
        "const e = new CustomEvent(\"x\", { detail: new D() });",
        "e.detail",
    );
    assert!(!k.top && k.objs.iter().any(|o| o.ends_with(":D")), "{k:?}");
}

#[test]
fn view_buffer_is_the_buffer_it_was_made_on() {
    let k = stored(
        "const b1: any = new ArrayBuffer(8); b1.tag = 1; const b2: any = new ArrayBuffer(8); b2.tag = new D(); const u: any = new Float64Array(b2);",
        "u.buffer.tag",
    );
    assert!(!k.top && k.objs.iter().any(|o| o.ends_with(":D")), "{k:?}");
}
