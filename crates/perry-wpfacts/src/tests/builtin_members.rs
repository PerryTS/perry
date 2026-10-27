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

fn unproven(pf: &ProgramFacts, key: &str) -> bool {
    let s: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    !s.is_empty() && s.iter().all(|s| matches!(s.verdict, Verdict::No(_)))
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

/// Soundness: a builtin prototype handed to unknown code may be written by
/// a store into ⊤ (`proto_in_top`); without that, a store into ⊤ under a
/// key is no write of the intrinsics.
#[test]
fn a_prototype_handed_to_unknown_code_makes_top_stores_writes_of_it() {
    let base = |hand: &str| {
        format!(
            "{CLASSES}{M}
            declare const unknown: any;
            class N {{ inner: any; constructor() {{ this.inner = new M(); }} }}
            {hand}
            const t: any = unknown.value;
            t.inner = function () {{ return 1; }};
            const v: any = k > 1 ? new N() : \"text\";
            const i = v.inner;
            if (i) console.log(i.m(new A()));
            console.log(new A().a);"
        )
    };
    let pf = facts(&base(""));
    assert!(proven(&pf, "m"), "{}", pf.dump_text());
    let pf = facts(&base("unknown.take(String.prototype);"));
    assert!(unproven(&pf, "m"), "{}", pf.dump_text());
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
/// the read), or onto a builtin prototype of its chain, is a member; a
/// constructor namespace handed to unknown code is reached along a chain, as
/// an instance's prototype is: no store into ⊤ writes its prototype for
/// that (a prototype handed over directly does:
/// `a_prototype_handed_to_unknown_code_makes_top_stores_writes_of_it`).
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
        (body(top, ""), true),
        (body(&format!("unknown.take(Map); {top}"), ""), true),
        (body(&format!("unknown.take(Map.prototype); {top}"), ""), false),
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
    for (src, want) in [
        (body("", ""), true),
        (body(w, ""), false),
        (body("", w), false),
    ] {
        let pf = facts(&src);
        assert_eq!(proven(&pf, "m"), want, "{src}\n{}", pf.dump_text());
    }
}

/// The runtime's own methods run on a builtin prototype (what a protocol
/// touch of it calls: the CommonJS glue's `getOwnPropertyDescriptor`)
/// give the prototype to no one: a store into ⊤ stays no
/// write of the intrinsics. Soundness: the prototype passed as an argument
/// to an opaque builtin is handed over.
#[test]
fn an_own_method_of_a_prototype_hands_nothing() {
    let body = |hand: &str| {
        format!(
            "{CLASSES}{M}
            declare const unknown: any;
            class N {{ inner: any; constructor() {{ this.inner = new M(); }} }}
            {hand}
            const t: any = unknown.value;
            t.inner = function () {{ return 1; }};
            const v: any = k > 1 ? new N() : \"text\";
            const i = v.inner;
            if (i) console.log(i.m(new A()));
            console.log(new A().a);"
        )
    };
    for (src, want) in [
        (body("(String.prototype as any).toString.call(\"x\"); console.log((String.prototype as any).concat(\"y\"));"), true),
        (body("unknown.take(String.prototype);"), false),
    ] {
        let pf = facts(&src);
        assert_eq!(proven(&pf, "m"), want, "{src}\n{}", pf.dump_text());
    }
}
