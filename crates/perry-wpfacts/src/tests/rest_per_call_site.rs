//! E2: a rest parameter's array is born by each call (ECMA-262 10.2.11
//! FunctionDeclarationInstantiation), so a function with a rest parameter
//! runs per call site: each call's `a[0]` is that call's first argument.
use super::*;

/// `Out.v` = the second call's `a[0]` (a Number), after a first call passed
/// a Tex; `pre` declares `first`, `call` names it.
fn second_arg(pre: &str, call: &str) -> crate::KindFact {
    let pf = facts(&format!(
        "class Tex {{ id = 1; }}
         class Out {{ v: any; constructor(v: any) {{ this.v = v; }} }}
         {pre}
         const t = {call}(new Tex());
         const o = new Out({call}(2));
         console.log(o.v, t);"
    ));
    field(tmpl(&pf, "Out"), "v").kind.clone()
}

fn number_only(v: crate::KindFact, what: &str) {
    assert!(v.objs.is_empty() && !v.top, "{what} {v:?}");
    assert!(v.prims & crate::model::prim::NUM != 0, "{what} {v:?}");
}

#[test]
fn rest_array_is_one_object_per_call_site() {
    number_only(
        second_arg("function first(...a: any[]): any { return a[0]; }", "first"),
        "function",
    );
}

#[test]
fn rest_array_of_a_method_call_is_per_call_site() {
    number_only(
        second_arg(
            "const w: any = {}; w.first = (...a: any[]): any => a[0];",
            "w.first",
        ),
        "arrow called as a method",
    );
}

#[test]
fn rest_array_of_a_returned_closure_is_per_call_site() {
    number_only(
        second_arg(
            "function pick(): any { return (...a: any[]): any => a[0]; } const g: any = pick();",
            "g",
        ),
        "returned closure",
    );
}
