//! Rule G2 (field_truthy_filter): inside the code that runs only when
//! `x.f` was truthy, the local `x` holds only the values whose read of `f`
//! can be truthy (ECMA-262 7.1.2 ToBoolean, 13.13 `&&`, 14.6 `if`). A
//! token is left out only when its read of `f` (own fields, the prototype
//! chain, the unknown-key field, accessors) holds only `undefined`/`null`;
//! three.js's `currentValue && currentValue.isColor` guard
//! (`Material.setValues`).
use super::*;

fn src(attr_extra: &str, test: &str, pre: &str) -> String {
    format!(
        "class Color {{ v: any; set(v: any) {{ this.v = v; }} }}
         (Color.prototype as any).isColor = true;
         class Attr {{ w: any; {attr_extra} set(v: any) {{ this.w = v; }} }}
         function apply(cur: any, val: any) {{ if ({test}) {{ cur.set(val); }} }}
         const a: any = new Attr();
         {pre}
         apply(new Color(), 1);
         apply(a, 2);
         console.log(a.w);"
    )
}

const TEST: &str = "cur && cur.isColor";

/// The receivers and targets of the `cur.set(val)` call.
fn set_call(pf: &ProgramFacts) -> (Vec<String>, Vec<String>) {
    let s = pf
        .sites
        .iter()
        .filter(|s| s.key == "set" && s.is_call)
        .last()
        .expect("the cur.set call");
    let mut r: Vec<String> = s
        .receivers
        .iter()
        .map(|r| r.rsplit(':').next().unwrap_or(r).to_string())
        .collect();
    r.sort();
    r.dedup();
    (r, s.targets.clone())
}

fn only_color(pf: &ProgramFacts) -> bool {
    let (r, t) = set_call(pf);
    r == vec!["Color".to_string()] && t.len() == 1
}

#[test]
fn truthy_flag_keeps_only_the_flagged_class() {
    let pf = facts(&src("", TEST, ""));
    let (r, t) = set_call(&pf);
    assert!(only_color(&pf), "receivers {r:?} targets {t:?}");
}

#[test]
fn negated_flag_filters_the_alternate() {
    let s = src("", TEST, "").replace(
        "if (cur && cur.isColor) { cur.set(val); }",
        "if (!cur.isColor) { } else { cur.set(val); }",
    );
    let pf = facts(&s);
    let (r, t) = set_call(&pf);
    assert!(only_color(&pf), "receivers {r:?} targets {t:?}");
}

#[test]
fn flag_under_unknown_key_passes() {
    let (r, _) = set_call(&facts(&src("", TEST, "a[process.argv[2]] = 1;")));
    assert!(r.contains(&"Attr".to_string()), "{r:?}");
}

#[test]
fn unknown_code_value_passes() {
    let s = src("", TEST, "").replace(
        "apply(a, 2);",
        "apply(a, 2); apply(new Function(process.argv[2])(), 3);",
    );
    let pf = facts(&s);
    let (r, t) = set_call(&pf);
    assert!(!only_color(&pf), "receivers {r:?} targets {t:?}");
}

#[test]
fn write_inside_the_guard_joins_back() {
    let s = src("", TEST, "").replace(
        "function apply(cur: any, val: any) { if (cur && cur.isColor) { cur.set(val); } }",
        "function apply(cur: any, val: any) { let c = cur; if (c && c.isColor) { c = val; c.set(val); } }",
    ).replace("apply(a, 2);", "apply(a, a);");
    let (r, _) = set_call(&facts(&s));
    assert!(r.contains(&"Attr".to_string()), "{r:?}");
}

#[test]
fn or_test_keeps_both() {
    let pf = facts(&src("", "cur.isColor || val", ""));
    let (r, _) = set_call(&pf);
    assert!(r.contains(&"Attr".to_string()), "{r:?}");
}

#[test]
fn accessor_flag_passes() {
    let pf = facts(&src("get isColor(): any { return process.argv.length; }", TEST, ""));
    let (r, _) = set_call(&pf);
    assert!(r.contains(&"Attr".to_string()), "{r:?}");
}

#[test]
fn null_flag_store_is_filtered_and_object_store_passes() {
    // `isColor` stored as null on Attr: falsy, filtered.
    let pf = facts(&src("isColor: any = null;", TEST, ""));
    assert!(only_color(&pf), "{:?}", set_call(&pf));
    // A number (truthiness unknown to the solver) passes.
    let pf = facts(&src("isColor: any = 0;", TEST, ""));
    assert!(set_call(&pf).0.contains(&"Attr".to_string()), "{:?}", set_call(&pf));
}

/// `Object.prototype.isColor = true` is an add to an add-locked prototype:
/// the runtime refuses it with a TypeError at the store (DEC 250,
/// `perry_abi::lock::refuses`, `builtin_proto::note_sealed_write`), so no
/// read of `isColor` past `Attr.prototype` finds it. The filter reads as
/// `cur.isColor` does there (`Cons::Load`, the final read of
/// `builtins::chain_load`): Attr is filtered, the store before or after
/// the guarded call in program order alike.
#[test]
fn object_prototype_add_is_refused_so_the_read_stays_final() {
    let pf = facts(&src("", TEST, "(Object.prototype as any).isColor = true;"));
    assert!(only_color(&pf), "{:?}", set_call(&pf));
    let s = src("", TEST, "").replace(
        "console.log(a.w);",
        "(Object.prototype as any).isColor = true; console.log(a.w);",
    );
    let pf = facts(&s);
    assert!(only_color(&pf), "{:?}", set_call(&pf));
}

/// Attr reaches the guarded call (or the call is not proven to Color).
fn passes(s: &str) {
    let pf = facts(s);
    let (r, t) = set_call(&pf);
    assert!(
        r.contains(&"Attr".to_string()) || !only_color(&pf),
        "receivers {r:?} targets {t:?}"
    );
}

#[test]
fn intermediate_prototype_flag_passes() {
    passes(&src("", TEST, "").replace(
        "class Attr {",
        "class Base { } (Base.prototype as any).isColor = 1; class Attr extends Base {",
    ));
}

#[test]
fn relinked_prototype_flag_passes() {
    // A program object linked under Attr.prototype, or under the instance.
    passes(&src("", TEST, "Object.setPrototypeOf(Attr.prototype, { isColor: true });"));
    passes(&src("", TEST, "Object.setPrototypeOf(a, { isColor: 1 });"));
    // Array.prototype takes named adds (DEC 250: not add-locked).
    passes(&src(
        "",
        TEST,
        "Object.setPrototypeOf(Attr.prototype, Array.prototype); (Array.prototype as any).isColor = true;",
    ));
}

#[test]
fn token_handed_to_unknown_code_passes() {
    passes(&src("", TEST, "new Function(process.argv[2])(a);"));
    passes(&src("", TEST, "new Function(process.argv[2])(Attr);"));
}

#[test]
fn unknown_key_store_on_the_prototype_passes() {
    passes(&src("", TEST, "(Attr.prototype as any)[process.argv[2]] = 1;"));
}

#[test]
fn named_store_into_top_passes_where_top_may_be_the_instance() {
    // The instance escaped: ⊤ may be it.
    passes(&src("", TEST, "(globalThis as any).keep = a; const u: any = (globalThis as any)[process.argv[2]]; u.isColor = true;"));
}

#[test]
fn named_store_into_top_passes_where_top_may_be_the_class() {
    // The class escaped: ⊤ may be it, and its prototype the store target.
    passes(&src("", TEST, "(globalThis as any).K = Attr; const u: any = (globalThis as any)[process.argv[2]]; u.prototype.isColor = true;"));
}

/// Nothing of Attr reached unknown code and the add to `Object.prototype`
/// is refused (DEC 250): a store into ⊤ cannot give an Attr `isColor`.
#[test]
fn named_store_into_top_without_a_path_to_the_token_filters() {
    let pf = facts(&src(
        "",
        TEST,
        "const u: any = (globalThis as any)[process.argv[2]]; u.isColor = true;",
    ));
    assert!(only_color(&pf), "{:?}", set_call(&pf));
}
