//! Element identity through containers: what a method or constructor of
//! object `o` births is `o`'s (a maker context), so a container class's
//! backing store is per container object, a container inside a container
//! per outer container, and a value read back out of a container is
//! exactly what was put into that container.

use super::*;

/// A container class whose store is born in its methods (the som.Vector
/// shape: `append` makes the array on first use and copies it on growth).
const VEC: &str = r#"
    class Vec {
        storage: any; n: number;
        constructor() { this.storage = null; this.n = 0; }
        append(e: any) {
            if (this.storage === null) { this.storage = new Array(4); }
            else if (this.n >= this.storage.length) { this.storage = this.storage.slice(); }
            this.storage[this.n] = e; this.n += 1;
        }
        at(i: number) { return this.storage[i]; }
        forEach(f: any) { for (let i = 0; i < this.n; i += 1) { f(this.storage[i]); } }
    }
"#;

/// A container holding its elements in another container, born in its
/// constructor (the som.Set shape over som.Vector).
const SET: &str = r#"
    class Bag {
        items: any;
        constructor() { this.items = new Vec(); }
        add(e: any) { this.items.append(e); }
        forEach(f: any) { this.items.forEach(f); }
    }
"#;

const CLASSES: &str = r#"
    class A { a = 1; }
    class B { b = "x"; }
"#;

fn sites<'a>(pf: &'a ProgramFacts, key: &str) -> Vec<&'a crate::SiteFacts> {
    let v: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    assert!(!v.is_empty(), "site {key} in\n{}", pf.dump_text());
    v
}

/// Every site of `key` holds exactly `want` (template displays), no ⊤.
fn holds(pf: &ProgramFacts, key: &str, want: &[&str]) {
    for s in sites(pf, key) {
        let mut w: Vec<String> = want.iter().map(|x| format!("t.ts:{x}")).collect();
        w.sort();
        let mut got = s.receivers.clone();
        got.sort();
        assert!(
            !s.top && got == w,
            "{key}: {:?} top={} want {w:?}\n{}",
            s.receivers,
            s.top,
            pf.dump_text()
        );
    }
}

#[test]
fn a_container_store_born_in_a_method_is_per_container() {
    let pf = facts(&format!(
        "{VEC}{CLASSES}
        const va = new Vec(); va.append(new A()); va.append(new A());
        const vb = new Vec(); vb.append(new B());
        const vc = new Vec(); vc.append(\"xy\".charAt(0));
        console.log(va.at(0).a, vb.at(0).b, vc.at(0));"
    ));
    holds(&pf, "a", &["A"]);
    holds(&pf, "b", &["B"]);
    let a = tmpl(&pf, "A");
    assert!(a.escape.is_none() && a.hard.is_none(), "{}", pf.dump_text());
}

#[test]
fn nested_containers_keep_their_elements_apart() {
    // Two bags of different element classes: each bag's vector is born in
    // that bag's constructor and each vector's store in that vector's
    // `append`, three levels of makers.
    let pf = facts(&format!(
        "{VEC}{SET}{CLASSES}
        const ba = new Bag(); ba.add(new A());
        const bb = new Bag(); bb.add(new B());
        ba.forEach((x: any) => console.log(x.a));
        bb.forEach((y: any) => console.log(y.b));"
    ));
    holds(&pf, "a", &["A"]);
    holds(&pf, "b", &["B"]);
}

#[test]
fn a_method_call_on_an_element_of_a_vector_of_vectors_is_proven() {
    // The cd.js shape: a vector of vectors of instances, a method of the
    // element called with another element; a vector of strings elsewhere in
    // the program does not reach it.
    let pf = facts(&format!(
        "{VEC}
        class M {{ p = 1; hit(o: any) {{ return o.p; }} }}
        function group(xs: any) {{ const out = new Vec(); out.append(xs); return out; }}
        const inner = new Vec(); inner.append(new M()); inner.append(new M());
        const all = group(inner);
        const words = new Vec(); words.append(\"ab\".charAt(1));
        all.forEach((r: any) => {{
            for (let i = 0; i < 2; i += 1) {{ console.log(r.at(i).hit(r.at(1))); }}
        }});
        console.log(words.at(0));"
    ));
    let hit = sites(&pf, "hit");
    for s in hit {
        assert_eq!(s.verdict, Verdict::T0Mono, "{}", pf.dump_text());
    }
    holds(&pf, "p", &["M"]);
    let m = tmpl(&pf, "M");
    assert!(m.escape.is_none(), "{}", pf.dump_text());
}

#[test]
fn a_shared_container_holds_what_every_alias_put_in() {
    // Differential: one vector reached through two names and a helper; a
    // read must hold every class any path stored (a claim of A alone would
    // be wrong at run time).
    let pf = facts(&format!(
        "{VEC}{CLASSES}
        class C {{ a = 3; b = \"z\"; }}
        function put(v: any, x: any) {{ v.append(x); }}
        const va = new Vec(); const vb = new Vec();
        const pick = (Date.now() > 0) ? va : vb;
        put(va, new A()); put(pick, new C());
        vb.append(new B());
        console.log(va.at(0).a, vb.at(0).b);"
    ));
    holds(&pf, "a", &["A", "C"]);
    holds(&pf, "b", &["B", "C"]);
}

#[test]
fn a_recursive_maker_folds_onto_its_ancestor() {
    // A list node births the next node in its own method: the chain of
    // makers would never end; the birth at a site already on the chain is
    // that ancestor again, so the solve ends and every value is seen.
    let pf = facts(&format!(
        "{CLASSES}
        class Node {{
            v: any; next: any;
            constructor(v: any) {{ this.v = v; this.next = null; }}
            add(v: any) {{ if (this.next === null) {{ this.next = new Node(v); }} else {{ this.next.add(v); }} }}
        }}
        const head = new Node(new A());
        head.add(new B()); head.add(new A());
        let n: any = head;
        while (n !== null) {{ console.log(n.v.a); n = n.next; }}"
    ));
    let s = sites(&pf, "a");
    for x in s {
        let mut got = x.receivers.clone();
        got.sort();
        assert!(
            !x.top && got == vec!["t.ts:A".to_string(), "t.ts:B".to_string()],
            "{:?}\n{}",
            x.receivers,
            pf.dump_text()
        );
    }
}

#[test]
fn results_of_results_do_not_chain_makers() {
    // A persistent structure: every method returns a new node of one of
    // three classes, from six sites. A method's receiver that a method made
    // stands for the object the chain started from, so the births stay
    // (site x origin), not one per path through the sites.
    let pf = facts(
        r#"
        declare const k: number;
        class P { v: any; constructor(v: any) { this.v = v; } m(x: any): any { return k > 1 ? new Q(x) : new R(this.v); } }
        class Q { v: any; constructor(v: any) { this.v = v; } m(x: any): any { return k > 2 ? new P(x) : new R(this.v); } }
        class R { v: any; constructor(v: any) { this.v = v; } m(x: any): any { return k > 3 ? new P(x) : new Q(this.v); } }
        let n: any = new P(1);
        for (let i = 0; i < 9; i++) { n = n.m(i); }
        console.log(n.v);
        "#,
    );
    // 33 tokens / 14 instances here; one maker per path: 59 / 66.
    assert!(
        pf.stats.tokens < 45 && pf.stats.instances < 40,
        "tokens {} instances {}\n{}",
        pf.stats.tokens,
        pf.stats.instances,
        pf.dump_text()
    );
    let all = ["t.ts:P", "t.ts:Q", "t.ts:R"];
    let last = sites(&pf, "v")
        .into_iter()
        .find(|s| !s.top && s.receivers.len() == 3);
    assert!(
        last.is_some_and(|s| all.iter().all(|w| s.receivers.iter().any(|r| r == w))),
        "{}",
        pf.dump_text()
    );
}

#[test]
fn an_alternating_persistent_structure_runs_once_per_class() {
    // Two node classes whose methods make each other from four sites each
    // (the HAMT shape: a node's update returns a node of another class).
    // Every node a method made births the same (site, origin) objects, so
    // its methods run as that origin, once per class: the method instances
    // are bounded by the classes, not by the nodes.
    let pf = facts(
        r#"
        declare const k: number;
        class P { v: any; constructor(v: any) { this.v = v; }
            m(x: any): any { return k > 1 ? new Q(x) : k > 2 ? new Q(this.v) : k > 3 ? new Q(1) : new Q("p"); } }
        class Q { v: any; constructor(v: any) { this.v = v; }
            m(x: any): any { return k > 1 ? new P(x) : k > 2 ? new P(this.v) : k > 3 ? new P(2) : new P("q"); } }
        let n: any = new P(1);
        for (let i = 0; i < 9; i++) { n = n.m(i); }
        console.log(n.v);
        "#,
    );
    assert!(
        pf.stats.instances < 40,
        "tokens {} instances {}\n{}",
        pf.stats.tokens,
        pf.stats.instances,
        pf.dump_text()
    );
}

#[test]
fn a_persistent_structures_classes_keep_their_reads_apart() {
    // Differential: the nodes of one structure run their methods as its
    // origin, in one instance across their classes, and an inherited
    // method still reads each node's own fields (a claim that `l2.get()`
    // may be the C the I nodes hold, or `i.get()` the A, is imprecise; a
    // claim of fewer is wrong at run time).
    let pf = facts(&format!(
        "{CLASSES}
        class C {{ a = 3; b = \"z\"; }}
        class N {{ k: any; constructor(k: any) {{ this.k = k; }} get(): any {{ return this.k; }} }}
        class L extends N {{ m(): any {{ return new I(new C()); }} }}
        class I extends N {{ m(): any {{ return new L(new A()); }} }}
        function walk(l: any) {{ const i = l.m(); const l2 = i.m(); console.log(l2.get().a, i.get().b); }}
        walk(new L(new A()));"
    ));
    holds(&pf, "a", &["A"]);
    holds(&pf, "b", &["C"]);
}

#[test]
fn a_recursive_constructor_folds_onto_its_ancestor() {
    // Construction nests (a part of a part is the whole's), so a
    // constructor that constructs its own class, or a class that constructs
    // it back, would nest forever: a site already on the chain is the
    // object born there.
    let pf = facts(&format!(
        "{CLASSES}
        class T {{ kid: any; v: any; constructor(n: number) {{ this.v = new A(); this.kid = n > 0 ? new U(n - 1) : null; }} }}
        class U {{ kid: any; v: any; constructor(n: number) {{ this.v = new B(); this.kid = n > 0 ? new T(n - 1) : null; }} }}
        let t: any = new T(5);
        while (t !== null) {{ console.log(t.v.a); t = t.kid; }}"
    ));
    let s = sites(&pf, "a");
    for x in s {
        let mut got = x.receivers.clone();
        got.sort();
        assert!(
            !x.top && got == vec!["t.ts:A".to_string(), "t.ts:B".to_string()],
            "{:?}\n{}",
            x.receivers,
            pf.dump_text()
        );
    }
}
