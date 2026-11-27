//! An outlined for-in body (`Gen::for_in_body`) runs in place, between the
//! enclosing code's tests: its read of an enclosing local sees the
//! narrowing a guard around the loop leaves (`Gen::narrowed_read`).
use super::*;

fn out_v(guard: &str) -> crate::KindFact {
    let pf = facts(&format!(
        "class Tex {{ id = 1; }}
         class Out {{ v: any; constructor(v: any) {{ this.v = v; }} }}
         const x: any = process.argv[2] ? new Tex() : null;
         const keys: any = {{ a: 1, b: 2 }};
         if ({guard}) {{
           for (const k in keys) {{ console.log(new Out(x).v, k); }}
         }}"
    ));
    field(tmpl(&pf, "Out"), "v").kind.clone()
}

#[test]
fn an_outlined_for_in_body_reads_the_enclosing_guards_narrowing() {
    let v = out_v("x != null");
    assert!(v.prims & crate::model::prim::NULL == 0 && !v.top, "x {v:?}");
    assert!(v.objs.iter().any(|o| o.ends_with(":Tex")), "x {v:?}");
}

#[test]
fn an_outlined_for_in_body_under_no_guard_reads_null_too() {
    let v = out_v("process.argv.length > 1");
    assert!(v.prims & crate::model::prim::NULL != 0, "control {v:?}");
}
