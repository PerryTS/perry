//! Realm getter forms: a PerformanceEntry field getter reads the entry's
//! own field (`field(this, K)`) and every declared entry birth defines the
//! fields; %ThrowTypeError% (`Function.prototype.caller`) returns no value
//! (`returns(never)`).
use super::*;
use crate::model::prim;

fn stored(prelude: &str, read: &str) -> crate::derive::KindFact {
    let pf = facts(&format!(
        "class H {{ v: any = 0; }}\nclass D {{ q = 1; }}\n{prelude}\nconst h = new H();\nh.v = {read};\nconsole.log(h.v);\n"
    ));
    field(tmpl(&pf, "H"), "v").kind.clone()
}

#[test]
fn a_mark_entry_holds_its_declared_fields() {
    let k = stored("const e: any = performance.mark(\"a\");", "e.name");
    assert!(!k.top && k.prims & prim::STR != 0 && k.prims & !(prim::STR | prim::NUM) == 0 && k.objs.is_empty(), "{k:?}");
    let k = stored("const e: any = performance.measure(\"m\");", "e.duration");
    assert!(!k.top && k.prims == prim::NUM && k.objs.is_empty(), "{k:?}");
    let k = stored("const e: any = performance.mark(\"a\");", "e.name.length");
    assert!(!k.top && k.prims == prim::NUM, "{k:?}");
}

#[test]
fn a_mark_field_the_program_rewrites_reads_what_it_stored() {
    let k = stored("const e: any = performance.mark(\"a\"); e.name = new D();", "e.name");
    assert!(!k.top && k.objs.iter().any(|o| o.ends_with(":D")), "{k:?}");
}

#[test]
fn throw_type_error_returns_no_value() {
    let k = stored(
        "let r: any = 0; try { r = (function () {} as any).caller; } catch (e) {}",
        "r",
    );
    assert!(!k.top && k.prims == prim::NUM && k.objs.is_empty(), "{k:?}");
}
