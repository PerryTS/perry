//! Rule G3 (field_test_narrowing): inside the clauses that
//! `switch (x.f)` selects by a `case` literal of a primitive type (String,
//! Number, `null`, `undefined`), and where `x.f === lit` is true, `x`
//! holds only the values whose read of `f` may yield a value of the
//! literal's type (ECMA-262 14.12.2 CaseClauseIsSelected, 7.2.16
//! IsStrictlyEqual step 1: values of different types are never strictly
//! equal; the read is the ordinary [[Get]] of 10.1.8, prototype chain,
//! getters and ⊤ included). three.js GLTFLoader `_loadLight`:
//! `lightDefs[lightIndex]` with an unknown index reads the Array.prototype
//! and Object.prototype methods, but `lightDef.spot = ...` sits in
//! `case 'spot':` of `switch (lightDef.type)`, and no built-in function
//! has a `type`.
use super::*;

const F: &str = "function f(defs: any, k: any) { const d = defs[k];
    switch (d.type) { case 'spot': d.spot = { m: 1 }; break; case 'point': break;
    default: throw new Error('x'); } }";

fn src(f: &str, extra: &str) -> String {
    format!(
        "class BoxP {{ v: any; constructor(v: any) {{ this.v = v; }} }}
         class BoxH {{ v: any; constructor(v: any) {{ this.v = v; }} }}
         class BoxE {{ v: any; constructor(v: any) {{ this.v = v; }} }}
         const defs: any = [{{ type: 'spot' }}];
         {extra}
         {f}
         f(defs, process.argv[2]);
         const p = new BoxP(([] as any).push.spot);
         const h = new BoxH(({{}} as any).hasOwnProperty.spot);
         const e = new BoxE(defs[0].spot);
         console.log(p.v, h.v, e.v);"
    )
}

/// Whether the class's `v` may hold an object (the stored `{ m: 1 }`).
fn holds_obj(pf: &ProgramFacts, class: &str) -> bool {
    let k = &field(tmpl(pf, class), "v").kind;
    k.top || !k.objs.is_empty()
}

/// (push.spot, hasOwnProperty.spot, element.spot) may hold the object.
fn run(s: &str) -> (bool, bool, bool) {
    let pf = facts(s);
    (holds_obj(&pf, "BoxP"), holds_obj(&pf, "BoxH"), holds_obj(&pf, "BoxE"))
}

#[test]
fn switch_case_skips_builtin_functions() {
    let (p, h, e) = run(&src(F, ""));
    assert!(e, "the element object must get `spot`");
    assert!(!p && !h, "no built-in function gets `spot`: push {p} hasOwnProperty {h}");
}

#[test]
fn stored_type_admits_the_function() {
    let (p, h, _) = run(&src(F, "(Array.prototype.push as any).type = 'spot';"));
    assert!(p, "push has type 'spot': it may get `spot`");
    assert!(!h, "hasOwnProperty has no type");
}

#[test]
fn unknown_key_store_is_conservative() {
    let (p, _, _) = run(&src(
        F,
        "(Array.prototype.push as any)[process.argv[3]] = process.argv[4];",
    ));
    assert!(p, "push.type may be any string");
}

#[test]
fn reassigned_in_clause_no_filter() {
    let f = F
        .replace("const d = defs[k];", "let d = defs[k];")
        .replace("case 'spot': d.spot", "case 'spot': d = ([] as any).push; d.spot");
    let (p, _, _) = run(&src(&f, ""));
    assert!(p, "the written value is not filtered");
}

#[test]
fn closure_written_local_no_filter() {
    let f = F.replace(
        "const d = defs[k];",
        "let d = defs[k]; const g = () => { d = ([] as any).push; }; g();",
    );
    let (p, _, _) = run(&src(&f, ""));
    assert!(p, "a local a closure writes is no host");
}

#[test]
fn grouped_cases_union() {
    let f = F.replace("case 'spot':", "case 'a': case 'spot':");
    let (p, h, _) = run(&src(&f, "(Array.prototype.push as any).type = 'a';"));
    assert!(p, "a token whose type is 'a' enters the grouped clause");
    assert!(!h, "hasOwnProperty has no type");
}

#[test]
fn fallthrough_from_a_body_no_filter() {
    let f = F.replace(
        "const d = defs[k];",
        "let d = defs[k];",
    )
    .replace("case 'spot':", "case 'b': d = ([] as any).push; case 'spot':");
    let (p, _, _) = run(&src(&f, ""));
    assert!(p, "the fallthrough brings the written push");
}

#[test]
fn if_strict_equal_then_branch() {
    let f = "function f(defs: any, k: any) { const d = defs[k];
        if (d.type === 'spot') { d.spot = { m: 1 }; } }";
    let (p, h, e) = run(&src(f, ""));
    assert!(e && !p && !h, "push {p} hasOwnProperty {h} element {e}");
}

#[test]
fn if_strict_equal_else_branch_no_filter() {
    let f = "function f(defs: any, k: any) { const d = defs[k];
        if (d.type === 'spot') { } else { d.spot = { m: 1 }; } }";
    let (p, _, _) = run(&src(f, ""));
    assert!(p, "the else branch is not filtered");
}

#[test]
fn number_literal_case() {
    let f = F.replace("switch (d.type)", "switch (d.n)").replace("case 'spot':", "case 3:");
    let (p, h, _) = run(&src(&f, "defs[0].n = 3;"));
    assert!(!p && !h, "push {p} hasOwnProperty {h}");
}

#[test]
fn getter_on_the_chain_is_conservative() {
    let (p, h, _) = run(&src(
        F,
        "Object.defineProperty(Function.prototype, 'type', { get() { return 'spot'; } });",
    ));
    assert!(p && h, "a getter may yield 'spot': push {p} hasOwnProperty {h}");
}
