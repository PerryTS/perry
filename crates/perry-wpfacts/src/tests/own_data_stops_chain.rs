//! ECMA-262 10.1.8.1 OrdinaryGet: a read of a key the object holds as an
//! own data property is that property's value; the lookup never reaches
//! the prototype chain, whose data values and accessors under that key
//! are hidden. three.js `Cache = { enabled: false, ... }`: `Cache.enabled`
//! is `false` only, whatever a store through an unknown value may have
//! written onto `Object.prototype` or a class prototype under `enabled`.
//! Only an own key every birth path creates as a data property, which no
//! delete, defineProperty or unknown code can have removed, stops it.
use super::*;
use crate::model::prim;

fn src(body: &str, read: &str) -> String {
    format!(
        "class Box {{ v: any; constructor(v: any) {{ this.v = v; }} }}
         {body}
         const b = new Box({read});
         console.log(b.v);"
    )
}

/// What `b.v` holds: an object, a function or ⊤; its primitive kinds.
fn result(s: &str) -> (bool, u16, String) {
    let pf = facts(s);
    let k = &field(tmpl(&pf, "Box"), "v").kind;
    (
        k.top || k.funcs > 0 || k.builtin_objs > 0 || !k.objs.is_empty(),
        k.prims,
        pf.dump_text(),
    )
}

fn only(s: &str, want: u16) {
    let (objs, prims, d) = result(s);
    assert!(
        !objs && prims & !want == 0,
        "objs {objs} prims {}\n{d}",
        prim::name(prims)
    );
}

fn has(s: &str, want: u16) {
    let (objs, prims, _) = result(s);
    assert!(objs || prims & want == want, "objs {objs} prims {}", prim::name(prims));
}

const CACHE: &str = "const Cache: any = { enabled: false, files: {},
    get(k: any) { if (this.enabled === false) return; return this.files; } };
    const u: any = process.argv.length > 9 ? Object.prototype : (globalThis as any)[process.argv[2]];
    u.enabled = true;";

#[test]
fn literal_own_key_hides_a_top_store() {
    only(&src(CACHE, "Cache.enabled"), prim::FALSE);
}

#[test]
fn literal_own_flag_closes_the_method() {
    only(&src(CACHE, "Cache.get(process.argv[3])"), prim::UNDEF);
}

const KIND: &str = "class P { kind: any; constructor() { this.kind = 'a'; } }
    (P.prototype as any).kind = 5;";

#[test]
fn constructor_own_key_hides_the_prototype_value() {
    only(&src(KIND, "new P().kind"), prim::STR);
}

#[test]
fn literal_own_key_hides_an_inherited_accessor() {
    only(
        &src(
            "class B { get v(): any { return {}; } }
             const o: any = { __proto__: B.prototype, v: 1 };",
            "o.v",
        ),
        prim::NUM,
    );
}

// Controls: the chain is consulted. (A declared field `k: any;` is an own
// property of every instance, `undefined` until stored: no control.)

#[test]
fn a_key_some_constructor_paths_assign_reads_the_chain() {
    has(
        &src(
            "class Q { constructor(c: boolean) { if (c) (this as any).k = 1; } }
             (Q.prototype as any).k = 'x';",
            "new Q(process.argv.length > 3).k",
        ),
        prim::NUM | prim::STR,
    );
}

const R: &str = "class R { k: any; constructor() { this.k = 1; } }
    (R.prototype as any).k = 'x';";

#[test]
fn a_deleted_key_reads_the_chain() {
    has(
        &src(
            &format!("{R} const r: any = new R(); if (process.argv.length > 3) delete r.k;"),
            "r.k",
        ),
        prim::NUM | prim::STR,
    );
}

#[test]
fn an_object_given_to_unknown_code_reads_the_chain() {
    has(
        &src(
            &format!("{R} const r: any = new R(); new Function(process.argv[2])(r);"),
            "r.k",
        ),
        prim::NUM | prim::STR,
    );
}

#[test]
fn an_own_accessor_reads_the_getter() {
    has(&src("const o: any = { get k() { return 'g'; } };", "o.k"), prim::STR);
}

#[test]
fn a_computed_key_reads_the_chain() {
    has(
        &src(&format!("{R} const r: any = new R();"), "r[process.argv[3]]"),
        prim::NUM | prim::STR,
    );
}
