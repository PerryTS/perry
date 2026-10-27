//! The members of the intrinsic prototypes decide what a builtin object or
//! a primitive reads under a name (`builtins::members`): a name no
//! intrinsic defines is absent, not an opaque builtin function, so a value
//! that may also be a string or an array keeps its facts through a read or
//! a call of its class's method; `Object.prototype`'s members run through
//! the chain; a key the program writes onto a builtin prototype reads the
//! opaque member again.

use super::*;
use crate::model::prim;

const CLASSES: &str = r#"
    class A { a = 1; }
    class B { b = "x"; }
    declare const k: number;
"#;

/// A class whose method reads its argument's `a`.
const M: &str = r#"
    class M { m(o: any) { return o.a; } }
"#;

fn proven(pf: &ProgramFacts, key: &str) -> bool {
    let s: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    !s.is_empty()
        && s.iter()
            .all(|s| !s.top && matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set))
}

/// A value that may be a string or a class instance: reading a key of the
/// class on it reads `undefined` on the string, not a builtin function, so
/// a method called on what was read is the class's method and the class
/// keeps its facts (tsc: `node.text` on `string | Identifier`).
#[test]
fn a_class_key_read_on_a_string_is_absent_not_a_builtin_function() {
    let pf = facts(&format!(
        "{CLASSES}{M}
        class N {{ inner: any; constructor() {{ this.inner = new M(); }} }}
        const v: any = k > 1 ? new N() : \"text\";
        const i = v.inner;
        if (i) console.log(i.m(new A()));
        const f = v.inner;
        if (typeof f === \"object\") {{ f.extra = new B(); }}
        console.log(new B().b);"
    ));
    assert!(proven(&pf, "m"), "{}", pf.dump_text());
    assert!(proven(&pf, "a"), "{}", pf.dump_text());
    let a = tmpl(&pf, "A");
    assert!(a.escape.is_none(), "{}", pf.dump_text());
    // A store into what the read holds is a store into M, not into an opaque
    // builtin function value: B keeps its facts.
    assert!(proven(&pf, "b"), "{}", pf.dump_text());
    assert!(tmpl(&pf, "B").escape.is_none(), "{}", pf.dump_text());
}

/// A value that may be an array or a class instance: a string method or a
/// class method called on it is no method of the array (`Array.prototype`
/// has no `split`, no `m`), so the array does not escape and its elements
/// keep their facts, and the argument of the class's method is not handed
/// to an unmodeled array method.
#[test]
fn a_method_array_prototype_lacks_does_not_escape_the_array() {
    let pf = facts(&format!(
        "{CLASSES}{M}
        const xs: any[] = [new A(), new A()];
        const v: any = k > 1 ? xs : (k > 2 ? \"a,b\" : new M());
        const parts = typeof v === \"string\" ? v.split(\",\") : [];
        console.log(parts.length);
        if (v instanceof M) console.log(v.m(new A()));
        console.log(xs[0].a);"
    ));
    assert!(proven(&pf, "a"), "{}", pf.dump_text());
    assert!(tmpl(&pf, "A").escape.is_none(), "{}", pf.dump_text());
}

/// ToPrimitive on an array or a Map (`arr == x`, `${arr}`) calls `valueOf`,
/// which is `Object.prototype`'s (the receiver itself), and `toString`:
/// neither is an unmodeled method of the container, so the elements keep
/// their facts.
#[test]
fn to_primitive_on_an_array_runs_object_prototype_members_through_the_chain() {
    let pf = facts(&format!(
        "{CLASSES}
        const xs = [new A()];
        const s = `${{xs}}` + (xs == k ? \"y\" : \"n\");
        console.log(s.length, xs[0].a);
        const mp = new Map([[1, new B()]]);
        console.log(`${{mp}}`, mp.get(1).b);"
    ));
    assert!(proven(&pf, "a"), "{}", pf.dump_text());
    assert!(proven(&pf, "b"), "{}", pf.dump_text());
}

/// Soundness: the members of the intrinsics stay opaque. A builtin method
/// read as a value and called through `Function.prototype.call` hands the
/// arguments to the runtime (`set.call(t, k, v)` stores), whether the name
/// is the family's own or `Object.prototype`'s.
#[test]
fn described_members_keep_their_opaque_effects() {
    let pf = facts(&format!(
        "{CLASSES}
        const mp = new Map(); const f: any = mp.set; const tgt: any = new Map();
        f.call(tgt, \"k\", new A());
        console.log(new A().a);"
    ));
    assert!(tmpl(&pf, "A").escape.is_some(), "{}", pf.dump_text());
    let pf = facts(&format!(
        "{CLASSES}
        const mp = new Map(); const h: any = mp.hasOwnProperty;
        h.call(new B(), \"k\");
        console.log(new B().b);"
    ));
    assert!(tmpl(&pf, "B").escape.is_some(), "{}", pf.dump_text());
}

/// Soundness: a key the program writes onto a builtin prototype is a member
/// of every builtin object and primitive from then on, found through the
/// write before or after the lookup; a computed key may be any key.
#[test]
fn a_key_written_onto_a_builtin_prototype_is_a_member_again() {
    let w = "(String.prototype as any).inner = function (this: any) { return this; };";
    let body = |w: &str, after: &str| {
        format!(
            "{CLASSES}{M}
            class N {{ inner: any; constructor() {{ this.inner = new M(); }} }}
            const v: any = k > 1 ? new N() : \"text\";
            {w}
            const i = v.inner;
            if (i) console.log(i.m(new A()));
            {after}
            console.log(new A().a);"
        )
    };
    for (src, want) in [
        (body(w, ""), false),
        (body("", ""), true),
        // The write after the lookup: replayed.
        (body("", w), false),
        // A computed key on a builtin prototype may be `inner`.
        (
            body(
                "declare const name: string; (Array.prototype as any)[name] = function () { return 1; };",
                "",
            ),
            false,
        ),
    ] {
        let pf = facts(&src);
        assert_eq!(proven(&pf, "m"), want, "{src}\n{}", pf.dump_text());
    }
}

/// Soundness: a store into ⊤ may write any builtin prototype under its key,
/// whatever reached unknown code: unknown code reaches every intrinsic
/// prototype without being handed one (`Object.getPrototypeOf` of a
/// namespace is `Function.prototype`, of the global object or of any
/// instance's prototype chain ends at `Object.prototype`, interpreted code
/// reads the globals) and gives it back as ⊤. The runnable twins are the
/// ABM2 fixtures F2-F9: compiled code writes what unknown code returned.
/// Only the key is precise: a store into ⊤ under another key leaves `inner`
/// absent, a computed one may be `inner`.
#[test]
fn a_store_into_top_writes_the_intrinsics_under_its_key() {
    let body = |hand: &str, w: &str, other: &str| {
        format!(
            "{CLASSES}{M}
            declare const unknown: any;
            class N {{ inner: any; constructor() {{ this.inner = new M(); }} }}
            const t: any = unknown.reach({hand});
            {w}
            const v: any = k > 1 ? new N() : {other};
            const i = v.inner;
            if (i) console.log(i.m(new A()));
            console.log(new A().a);"
        )
    };
    let named = "t.inner = function () { return 1; };";
    let elsewhere = "t.unrelated = function () { return 1; };";
    let computed = "declare const kk: string; t[kk] = function () { return 1; };";
    for hand in [
        "0",
        "Map",
        "globalThis",
        "new Map()",
        "new B()",
        "function () {}",
        "Map.prototype",
    ] {
        // A program object's lookup ends at `Object.prototype` too (ABM2
        // fixtures FE/FF: a miscompile on main, fixed by the static rule).
        for other in ["Object", "\"text\"", "new Map()", "[1]", "new A()", "{ q: 1 }"] {
            for (w, want) in [(named, false), (computed, false), (elsewhere, true)] {
                let src = body(hand, w, other);
                let pf = facts(&src);
                assert_eq!(proven(&pf, "m"), want, "{src}\n{}", pf.dump_text());
            }
        }
    }
}

/// A namespace's prototype is a builtin prototype (`Function.prototype`
/// for a constructor, `Object.prototype` at the end of the global object's
/// chain): a store into what `Object.getPrototypeOf` gives writes the
/// intrinsics under its key. (Was: no prototype at all, so the store wrote
/// nothing and the read below stayed proven: fixtures F2/F3 of ABM2 with
/// the helper module visible.)
#[test]
fn the_prototype_of_a_namespace_is_a_builtin_prototype() {
    for (hand, other) in [("Map", "Object"), ("globalThis", "\"text\"")] {
        let src = format!(
            "{CLASSES}{M}
            class N {{ inner: any; constructor() {{ this.inner = new M(); }} }}
            const p: any = Object.getPrototypeOf({hand});
            p.inner = function () {{ return 1; }};
            const v: any = k > 1 ? new N() : {other};
            const i = v.inner;
            if (i) console.log(i.m(new A()));
            console.log(new A().a);"
        );
        let pf = facts(&src);
        assert!(!proven(&pf, "m"), "{src}\n{}", pf.dump_text());
    }
}

/// The rest parameter of a function unknown code calls is the fresh array
/// the call makes: a store into its elements is no store into ⊤, so it
/// writes no member of the intrinsics. (Was: the rest local held ⊤, and the
/// store made every key of every builtin prototype written: tsc's escaped
/// `(...args)` functions.)
#[test]
fn a_store_into_the_rest_array_of_an_escaped_function_writes_no_intrinsic() {
    let src = format!(
        "{CLASSES}{M}
        declare const kk: string;
        class N {{ inner: any; constructor() {{ this.inner = new M(); }} }}
        (globalThis as any).h = function (...xs: any[]) {{ xs[kk] = function () {{ return 1; }}; return xs.length; }};
        const v: any = k > 1 ? new N() : \"text\";
        const i = v.inner;
        if (i) console.log(i.m(new A()));
        console.log(new A().a);"
    );
    let pf = facts(&src);
    assert!(proven(&pf, "m"), "{src}\n{}", pf.dump_text());
}

/// A primitive's own members keep their kinds: `length` is a Number on a
/// string and absent on a number, `valueOf` is the primitive, `toString` a
/// string.
#[test]
fn primitive_members_have_their_kinds() {
    let pf = facts(
        r#"
        declare const k: number;
        const s = ["a", "b"].join(""); const n = Math.max(1, 2); const u: any = k > 1 ? s : n;
        class K { a: any; b: any; c: any; d: any; e: any;
          constructor() { this.a = s.length; this.b = s.valueOf(); this.c = n.toString(16); this.d = u.length; this.e = n.toFixed(2); } }
        const x = new K();
        console.log(x.a, x.b, x.c, x.d, x.e);
        "#,
    );
    let t = tmpl(&pf, "K");
    assert_eq!(field(t, "a").kind.desc(), "num", "{}", pf.dump_text());
    assert_eq!(field(t, "b").kind.desc(), "str", "{}", pf.dump_text());
    assert_eq!(field(t, "c").kind.desc(), "str", "{}", pf.dump_text());
    assert_eq!(
        field(t, "d").kind.prims,
        prim::NUM | prim::UNDEF,
        "{}",
        pf.dump_text()
    );
    assert!(!field(t, "e").kind.top, "{}", pf.dump_text());
}

/// A value that may be a class instance or a builtin namespace (tsc: the
/// `{Object, Function}` a computed read through `Object.prototype` gives):
/// a key no namespace static or `Function.prototype` member names is
/// absent, so the class's method is called and a store into what was read
/// is a store into the class instance, not into a builtin function value.
#[test]
fn a_class_key_read_on_a_namespace_is_absent() {
    let pf = facts(&format!(
        "{CLASSES}{M}
        class N {{ inner: any; constructor() {{ this.inner = new M(); }} }}
        const v: any = k > 1 ? new N() : (k > 2 ? Object : Function);
        const i = v.inner;
        if (i) {{ console.log(i.m(new A())); i.extra = new B(); }}
        console.log(new B().b, Math.PI + Number.MAX_SAFE_INTEGER);"
    ));
    assert!(proven(&pf, "m"), "{}", pf.dump_text());
    assert!(proven(&pf, "a"), "{}", pf.dump_text());
    assert!(tmpl(&pf, "B").escape.is_none(), "{}", pf.dump_text());
}

/// Soundness: a key the program stores onto a namespace (before or after
/// the read), onto a builtin prototype of its chain, or into ⊤ (which may be
/// `Function.prototype`: `a_store_into_top_writes_the_intrinsics_under_its_key`)
/// is a member.
#[test]
fn a_key_written_onto_a_namespace_is_a_member() {
    let body = |before: &str, after: &str| {
        format!(
            "{CLASSES}{M}
            declare const unknown: any;
            class N {{ inner: any; constructor() {{ this.inner = new M(); }} }}
            {before}
            const v: any = k > 1 ? new N() : Object;
            const i = v.inner;
            if (i) console.log(i.m(new A()));
            {after}
            console.log(new A().a);"
        )
    };
    let fw = "(Function.prototype as any).inner = function () { return 1; };";
    let top = "const t: any = unknown.value; t.inner = function () { return 1; };";
    for (src, want) in [
        (body("", ""), true),
        (body("(Object as any).inner = function () { return 1; };", ""), false),
        (body("", "(Object as any).inner = function () { return 1; };"), false),
        (body(fw, ""), false),
        (body(top, ""), false),
        (body(&format!("unknown.take(Map); {top}"), ""), false),
        (body("", top), false),
    ] {
        let pf = facts(&src);
        assert_eq!(proven(&pf, "m"), want, "{src}\n{}", pf.dump_text());
    }
}

/// The global object's runtime functions called as its methods
/// (`global.gc()`, tsc's `sys.gc`) ignore `this`: the call gives the global
/// object to no one, so a store into ⊤ stays no write of a program-owned
/// global. Soundness: a global function the program writes (before or after
/// the call) is an unknown function that receives the global object.
#[test]
fn a_runtime_global_function_called_on_the_global_object_hands_nothing() {
    let body = |before: &str, after: &str| {
        format!(
            "{CLASSES}{M}
            declare const unknown: any;
            {before}
            const g: any = globalThis;
            g.__perry_t_held = new M();
            if (g.gc) g.gc();
            const t: any = unknown.value;
            t.__perry_t_held = function () {{ return 1; }};
            console.log(g.__perry_t_held.m(new A()));
            {after}
            console.log(new A().a);"
        )
    };
    let w = "(globalThis as any).gc = unknown.f;";
    // ABM3 (static rule, CWN 10-27): the store into ⊤ under
    // `__perry_t_held` writes that key of `Object.prototype`, which the
    // global object's lookup reaches beside its own field (the model holds
    // no flow-sensitive "own key present"): the read is unknown with or
    // without the global object handed over. What the call hands over still
    // decides the global field itself.
    for src in [body("", ""), body(w, ""), body("", w)] {
        let pf = facts(&src);
        assert!(!proven(&pf, "m"), "{src}\n{}", pf.dump_text());
    }
    let held = |src: &str| {
        let pf = facts(src);
        pf.sites
            .iter()
            .filter(|s| s.key == "m")
            .any(|s| s.receivers.iter().any(|r| r.ends_with(":M")))
    };
    assert!(held(&body("", "")), "the global field keeps its M");
}

/// Soundness (ABM2 fixtures FB/FC, CWN 10-27): a store into ⊤ under the key
/// of a described builtin method (`Map.prototype.get`) may replace it: the
/// method read or called runs what was stored, beside its description. A
/// store under another key leaves the description alone.
#[test]
fn a_store_into_top_replaces_a_described_member_under_its_key() {
    let body = |w: &str| {
        format!(
            "{CLASSES}
            declare const unknown: any;
            class Q {{ x = 5; b = 1; }}
            const t: any = unknown.reach(Map);
            {w}
            const mp = new Map<number, any>(); mp.set(1, new B());
            const g = mp.get(1);
            if (g) console.log(g.b);
            const s = \"abc\".charAt(0);
            console.log(s.length);"
        )
    };
    let site_b = |pf: &ProgramFacts| {
        pf.sites
            .iter()
            .filter(|s| s.key == "b")
            .all(|s| !s.top && matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set))
    };
    let pf = facts(&body("t.unrelated = function () { return new Q(); };"));
    assert!(site_b(&pf), "{}", pf.dump_text());
    for w in [
        "t.get = function () { return new Q(); };",
        "declare const kk: string; t[kk] = function () { return new Q(); };",
    ] {
        let pf = facts(&body(w));
        assert!(!site_b(&pf), "{w}\n{}", pf.dump_text());
    }
}
