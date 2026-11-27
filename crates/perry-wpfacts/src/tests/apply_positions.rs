//! `f.apply(thisArg, argArray)` passes `argArray[i]` as argument `i`
//! (ECMA-262 20.2.3.1 step 4, 7.3.20 CreateListFromArrayLike): a rest
//! array forwarded by `apply` gives each parameter its own argument.
use super::*;

fn out(src: &str) -> [crate::KindFact; 2] {
    let pf = facts(&format!(
        "class Tex {{ id = 1; }}
         class Out {{ x: any; y: any; constructor(x: any, y: any) {{ this.x = x; this.y = y; }} }}
         {src}
         console.log(o.x, o.y);"
    ));
    let t = tmpl(&pf, "Out");
    [field(t, "x").kind.clone(), field(t, "y").kind.clone()]
}

#[test]
fn apply_passes_each_index_to_its_parameter() {
    let [x, y] = out(
        "function g(x: any, y: any): any { return new Out(x, y); }
         function w(...a: any[]): any { return g.apply(null, a); }
         const o = w(1, new Tex());",
    );
    assert!(x.objs.is_empty() && !x.top, "x {x:?}");
    assert!(y.prims & crate::model::prim::NUM == 0 && !y.top, "y {y:?}");
}

#[test]
fn control_apply_into_a_rest_parameter_merges_positions() {
    let [x, _] = out(
        "function g(x: any, ...r: any[]): any { return new Out(x, r[0]); }
         function w(...a: any[]): any { return g.apply(null, a); }
         const o = w(1, new Tex());",
    );
    assert!(x.objs.iter().any(|o| o.ends_with(":Tex")), "control x {x:?}");
}
