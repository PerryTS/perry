//! DEFDATA4 fixture (r7: three.js classes that define `id` on `this` in
//! the constructor, `clone() { return new this.constructor().copy(this) }`,
//! `copy` cloning a child of the other class). NAMED-DATA-DEFINE says the
//! definition is the store of that one key (ECMA-262 10.1.6.3). The store
//! form allocates the same descriptor literal, so both programs allocate the
//! same objects: they must solve to the same tokens and contexts.
use super::*;

fn prog(def: &str) -> String {
    prog_with(def, "a.clone()")
}

fn prog_with(def: &str, second: &str) -> String {
    let class = |me: &str, other: &str| {
        format!(
            "class {me} {{
               v: V; m: M; child: {other} | null = null;
               constructor() {{ this.v = new V(); this.m = new M(); {def} }}
               clone(): {me} {{ return new (this.constructor as any)().copy(this); }}
               clone2(): {me} {{ return new (this.constructor as any)().copy(this); }}
               copy(s: {me}) {{ this.v.copy(s.v); this.m.copy(s.m); this.child = s.child ? s.child.clone() : null; return this; }}
             }}"
        )
    };
    format!(
        "let n = 0;
         class V {{ x = 0; copy(o: V) {{ this.x = o.x; return this; }} }}
         class M {{ e: number[] = [1, 0]; copy(o: M) {{ this.e[0] = o.e[0]; return this; }} }}
         {a}
         {b}
         const a = new A(); const b = new B(); a.child = b; b.child = new A();
         const c1 = a.clone();
         const c2 = b.clone();
         const c5 = {second};
         const c3 = c1.clone();
         const c4 = (c2.child as A).clone();
         console.log(c1.v.x, c2.m.e[0], c3.child, c4.child, c5.v.x, (c4 as any).id);",
        a = class("A", "B"),
        b = class("B", "A"),
        second = second
    )
}

const DEFINE: &str = "Object.defineProperty(this, 'id', { value: n++ });";
const STORE: &str = "const d = { value: n++ }; (this as any).id = d.value;";

#[test]
fn a_defined_id_solves_as_its_store() {
    let d = facts(&prog(DEFINE));
    let s = facts(&prog(STORE));
    eprintln!(
        "DEFINE tokens={} contexts={} work={} | STORE tokens={} contexts={} work={}",
        d.stats.tokens, d.stats.contexts, d.stats.work, s.stats.tokens, s.stats.contexts, s.stats.work
    );
    assert_eq!((d.stats.tokens, d.stats.contexts), (s.stats.tokens, s.stats.contexts));
}

/// Control: the store form alone, printed on both heads (the count must
/// not move between 9008e11e10 and the rule).
#[test]
fn store_form_control() {
    let s = facts(&prog(STORE));
    eprintln!("CONTROL STORE tokens={} contexts={} work={}", s.stats.tokens, s.stats.contexts, s.stats.work);
}

/// The rule: what a constructor run of a method-made object births is keyed
/// by the origin of that object's chain of method calls (as `maker_key` keys
/// a method-made receiver). `a.clone2()` makes one more object at a second
/// site in `a`'s methods; its constructor's V, M, M's array and descriptor
/// are `a`'s (the same sites, the same origin), so the program has exactly
/// one token more than with a second `a.clone()`: the clone itself.
#[test]
fn a_ctor_run_of_a_method_made_object_births_as_its_origin() {
    for def in [DEFINE, STORE] {
        let one = facts(&prog_with(def, "a.clone()"));
        let two = facts(&prog_with(def, "a.clone2()"));
        eprintln!("ORIGIN {def}: one-site tokens={} two-site tokens={}", one.stats.tokens, two.stats.tokens);
        assert_eq!(two.stats.tokens, one.stats.tokens + 1, "{def}");
    }
}
