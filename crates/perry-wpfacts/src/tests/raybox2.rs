//! RAYBOX2. Rule 3 (prototype data stored after the class): a read
//! `inst.K` whose holder `C.prototype` gets `K` by exactly one top-level
//! store in the class's module, placed so that no instance of `C` (or of a
//! subclass) can exist before it runs, yields that store's values and not
//! `undefined` (ECMA-262 10.1.8.1 OrdinaryGet walks the chain; the store
//! ran before any receiver existed). Rule G2-false: on the false side of a
//! truthy test `x.f`, `x` keeps only the values whose read of `f` may be
//! falsy (7.1.2 ToBoolean).
use super::*;
use crate::model::prim;

/// `Box.v`'s primitive kinds and whether it holds an object.
fn boxv(s: &str) -> (u16, bool, ProgramFacts) {
    let pf = facts(s);
    let k = field(tmpl(&pf, "Box"), "v").kind.clone();
    (k.prims, k.top || !k.objs.is_empty() || k.funcs > 0, pf)
}

fn src(statics: &str, after: &str, read: &str) -> String {
    format!(
        "class Box {{ v: any; constructor(v: any) {{ this.v = v; }} }}
         function mk(): any {{ return new C(); }}
         class C {{ x: number; {statics} static {{ (C.prototype as any).K = true; }} constructor() {{ this.x = 1; }} }}
         {after}
         const c: any = new C();
         const b = new Box({read});
         console.log(b.v, c.x);"
    )
}

fn only_true(s: &str) {
    let (p, o, pf) = boxv(s);
    assert!(p == prim::TRUE && !o, "prims {}\n{}", prim::name(p), pf.dump_text());
}

fn has(s: &str, bits: u16) {
    let (p, _, pf) = boxv(s);
    assert!(p & bits == bits, "prims {} lacks {}\n{}", prim::name(p), prim::name(bits), pf.dump_text());
}

#[test]
fn r3_static_block_store_is_the_read() {
    only_true(&src("", "", "c.K"));
}

#[test]
fn r3_after_literal_statics() {
    only_true(&src("static z = 3; static { (this.prototype as any).L = 1; }", "", "c.K"));
}

#[test]
fn r3_keep_instance_made_before_the_store() {
    has(&src("static early: any = 0; static { C.early = new C(); }", "", "C.early.K"), prim::UNDEF);
}

#[test]
fn r3_keep_static_call_constructs() {
    has(
        &src("static m: any = mk();", "", "C.m.K"),
        prim::UNDEF,
    );
}

#[test]
fn r3_keep_second_store() {
    has(
        &src("", "function f() { (C.prototype as any).K = 0; } if (process.argv[2]) f();", "c.K"),
        prim::NUM | prim::TRUE,
    );
}

#[test]
fn r3_keep_delete() {
    has(&src("", "if (process.argv[2]) delete (C.prototype as any).K;", "c.K"), prim::UNDEF | prim::TRUE);
}

#[test]
fn r3_keep_instance_store() {
    has(&src("", "", "(process.argv[2] ? (c.K = 5, c) : c).K"), prim::NUM | prim::TRUE);
}

#[test]
fn r3_keep_subclass_own() {
    has(
        &src(
            "",
            "class D extends C { constructor() { super(); } } (D.prototype as any).K = 0;",
            "(process.argv[2] ? new D() : c as any).K",
        ),
        prim::NUM | prim::TRUE,
    );
}

/// Rule G2-false: `look(x)` takes the `else` arm only with a value whose
/// read of `isV` may be falsy; `t.x` is what `set` stored there. A
/// Number passes the true side too (its wrapper read is not judged), so
/// `copy` may read `undefined` from it.
fn g2(proto: &str, extra_call: &str) -> String {
    format!(
        "class Box {{ v: any; constructor(v: any) {{ this.v = v; }} }}
         class V {{ x: any; {proto} constructor() {{ this.x = 0; }}
           set(x: any) {{ this.x = x; return this; }} copy(v: any) {{ this.x = v.x; return this; }} }}
         const t = new V();
         function look(x: any) {{ if (x.isV) t.copy(x); else t.set(x); }}
         look(new V());
         look(Number(process.argv[2]));
         {extra_call}
         const b = new Box(t.x);
         console.log(b.v);"
    )
}

fn g2_obj(s: &str) -> bool {
    let (_, o, _) = boxv(s);
    o
}

#[test]
fn g2f_else_arm_has_no_flagged_instance() {
    let s = g2("static { (V.prototype as any).isV = true; }", "");
    let (p, o, pf) = boxv(&s);
    assert!(!o && p & !(prim::NUM | prim::UNDEF) == 0, "prims {} obj {o}\n{}", prim::name(p), pf.dump_text());
}

#[test]
fn g2f_keep_flag_from_argv() {
    let s = g2("", "(V.prototype as any).isV = process.argv[2];");
    assert!(g2_obj(&s), "a V may take the else arm");
}

#[test]
fn g2f_keep_literal_with_false_flag() {
    let s = g2("static { (V.prototype as any).isV = true; }", "look({ isV: false, x: 1 });");
    assert!(g2_obj(&s), "the literal takes the else arm");
}

#[test]
fn g2f_keep_getter_flag() {
    let s = g2("get isV(): any { return Math.random() > 2; }", "");
    assert!(g2_obj(&s), "the getter may give false");
}

#[test]
fn g2f_not_form_true_side() {
    let s = g2("static { (V.prototype as any).isV = true; }", "")
        .replace("if (x.isV) t.copy(x); else t.set(x);", "if (!x.isV) { t.set(x); return; } t.copy(x);");
    let (p, o, pf) = boxv(&s);
    assert!(!o && p & !(prim::NUM | prim::UNDEF) == 0, "prims {} obj {o}\n{}", prim::name(p), pf.dump_text());
}

/// RAYBOX3, the derive side of rule 3 (`constant_proto_members`): a read
/// of a key a static block stores on the prototype before any instance
/// exists is a constant prototype member (Rule B3 for an instance, the
/// prototype receiver for `C.prototype.K`). The verdicts of the read sites
/// of `key` (stores left out).
fn read_verdicts(pf: &ProgramFacts, key: &str) -> Vec<crate::Verdict> {
    let v: Vec<crate::Verdict> = pf
        .sites
        .iter()
        .filter(|s| s.key == key && !s.access.is_store())
        .map(|s| s.verdict.clone())
        .collect();
    assert!(!v.is_empty(), "no read site .{key}\n{}", pf.dump_text());
    v
}

fn proven(s: &str) {
    let pf = facts(s);
    let v = read_verdicts(&pf, "K");
    assert!(
        v.iter().all(|v| !matches!(v, crate::Verdict::No(_))),
        "{v:?}\n{}",
        pf.dump_text()
    );
}

fn not_proven(s: &str) {
    let pf = facts(s);
    let v = read_verdicts(&pf, "K");
    assert!(
        v.iter().any(|v| matches!(v, crate::Verdict::No(_))),
        "{v:?}\n{}",
        pf.dump_text()
    );
}

#[test]
fn d3_instance_read_is_proven() {
    proven(&src("", "", "c.K"));
}

#[test]
fn d3_prototype_read_is_proven() {
    proven(&src("", "", "(C.prototype as any).K"));
}

#[test]
fn d3_keep_second_writer() {
    not_proven(&src("", "function f() { (C.prototype as any).K = 0; } if (process.argv[2]) f();", "c.K"));
}

#[test]
fn d3_keep_delete() {
    not_proven(&src("", "if (process.argv[2]) delete (C.prototype as any).K;", "c.K"));
}

#[test]
fn d3_keep_instance_own() {
    not_proven(&src("", "", "(process.argv[2] ? (c.K = 5, c) : c).K"));
}

#[test]
fn d3_keep_accessor() {
    not_proven(&src(
        "",
        "if (process.argv[2]) Object.defineProperty(C.prototype, \"K\", { get() { return 7; } });",
        "c.K",
    ));
}

#[test]
fn d3_keep_set_prototype_of() {
    not_proven(&src("", "", "(process.argv[2] ? Object.setPrototypeOf(c, { K: 2 }) : c).K"));
}

/// A subclass's static block stores its own `K`: the site's value set is
/// the union of both constants. (Not proven yet: the solve lets the store
/// `D.prototype.K = 2` reach `C.prototype` too, a foreign writer of C's
/// `K`; RAYBOX3 open item.)
#[test]
fn d3_subclass_static_block_both_constants() {
    let s = src(
        "",
        "class D extends C { static { (D.prototype as any).K = 2; } constructor() { super(); } }",
        "(process.argv[2] ? new D() : c as any).K",
    );
    has(&s, prim::NUM | prim::TRUE);
}

/// Rule G2-true for primitive receivers: on the true side of `x.key`, a
/// primitive `x` stays only if its read of `key` (7.3.2 GetV: the wrapper
/// chain, whatever the program stored there) may be truthy.
fn g2t(key: &str, proto: &str, extra: &str, arg: &str) -> String {
    format!(
        "class Box {{ v: any; constructor(v: any) {{ this.v = v; }} }}
         class V {{ x: any; {proto} constructor() {{ this.x = 0; }}
           set(x: any) {{ this.x = x; return this; }} copy(v: any) {{ this.x = v.x; return this; }} }}
         const t = new V();
         function look(x: any) {{ if (x.{key}) t.copy(x); else t.set(x); }}
         look(new V());
         look({arg});
         {extra}
         const b = new Box(t.x);
         console.log(b.v);"
    )
}

const ISV: &str = "static { (V.prototype as any).isV = true; }";

#[test]
fn g2t_number_leaves_true_side() {
    let s = g2t("isV", ISV, "", "Number(process.argv[2])");
    let (p, o, pf) = boxv(&s);
    assert!(!o && p == prim::NUM, "prims {} obj {o}\n{}", prim::name(p), pf.dump_text());
}

#[test]
fn g2t_keep_builtin_member() {
    has(&g2t("toFixed", ISV, "", "Number(process.argv[2])"), prim::UNDEF);
}

#[test]
fn g2t_keep_number_prototype_store() {
    has(&g2t("isV", ISV, "(Number.prototype as any).isV = true;", "Number(process.argv[2])"), prim::UNDEF);
}

#[test]
fn g2t_keep_object_prototype_computed_store() {
    has(&g2t("isV", ISV, "(Object.prototype as any)[process.argv[2]] = 1;", "Number(process.argv[2])"), prim::UNDEF);
}

#[test]
fn g2t_keep_string_length() {
    has(&g2t("length", ISV, "", "String(Math.random())"), prim::UNDEF);
}
