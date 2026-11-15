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
