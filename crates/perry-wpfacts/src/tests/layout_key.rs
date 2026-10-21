//! Layout as the unit of propagation (CWL, secret-tests
//! CW_LAYOUT_CONTEXT_2026-10-20.md): a summary instance is keyed by the
//! receiver's layout, never by the call site that reaches it; the birth of
//! an object runs per birth site; a receiver node binds per token.

use super::*;

const CLASSES: &str = r#"
    class A { x = 1; a = 1; }
    class B { y = 2; b = 2; }
"#;

fn exact(pf: &ProgramFacts, key: &str, n: usize) {
    let v: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    assert!(!v.is_empty(), "site {key} in\n{}", pf.dump_text());
    for s in v {
        assert!(!s.top && s.receivers.len() == n, "{key}: {:?} top={}\n{}", s.receivers, s.top, pf.dump_text());
    }
}

#[test]
fn method_instances_are_per_layout_not_per_site() {
    // Thirty call sites of one method on one layout: one instance of `get`
    // (and one birth run of the field initializer), not one per site.
    let mut src = String::from("class P { v = 1; get() { return this.v; } }\nconst p = new P();\n");
    for _ in 0..30 {
        src.push_str("console.log(p.get());\n");
    }
    let pf = facts(&src);
    assert!(pf.stats.instances <= 3, "{}", pf.dump_text());
    let site = pf.sites.iter().find(|s| s.key == "v" && !s.is_call).expect("site");
    assert_eq!(site.verdict, Verdict::T0Mono, "{}", pf.dump_text());
}

#[test]
fn a_receiver_node_binds_per_layout() {
    // `get.call(h)` hands the receiver over as a node: each token binds the
    // instance of its own layout, so the two results stay apart.
    let pf = facts(&format!(
        "{CLASSES}
        class H1 {{ v = new A(); }}
        class H2 {{ v = new B(); }}
        const get = function (this: any) {{ return this.v; }};
        const ra = get.call(new H1()); const rb = get.call(new H2());
        console.log(ra.a, rb.b);"
    ));
    exact(&pf, "a", 1);
    exact(&pf, "b", 1);
}

#[test]
fn a_birth_runs_per_birth_site() {
    // Two literals of one shape are one layout; the constructor that fills
    // each one's fields runs per birth site, so the fields stay apart.
    let pf = facts(&format!(
        "{CLASSES}
        const p = {{ k: new A() }}; const q = {{ k: new B() }};
        console.log(p.k.a, q.k.b);"
    ));
    exact(&pf, "a", 1);
    exact(&pf, "b", 1);
}

#[test]
fn object_spread_copies_fields_by_key() {
    // `{...s}` copies each own key of `s` under the same key: `c.x` holds
    // what `s.x` holds, not every value of every key of `s`.
    let pf = facts(&format!(
        "{CLASSES}
        const s = {{ x: new A(), y: new B() }};
        const c = {{ ...s, z: 1 }};
        console.log(c.x.a, c.y.b);"
    ));
    exact(&pf, "a", 1);
    exact(&pf, "b", 1);
}

#[test]
fn only_a_generator_step_closure_is_its_own_iterator() {
    // Iterating a node that holds an ordinary function value does not call
    // that function with itself as `this` (only a transformed generator's
    // step closure is the generator object).
    let pf = facts(
        "const o = { k: 1 };
         const m = function (this: any) { return this.k; };
         const key: any = 'x';
         (m as any)[key]();
         console.log(m.call(o));",
    );
    // The computed method is unknown (the receiver escapes: `this` is ⊤
    // besides `o`), but `m` itself is not a receiver.
    for s in pf.sites.iter().filter(|s| s.key == "k") {
        assert!(s.receivers.iter().all(|r| !r.starts_with("fn:")), "{:?}\n{}", s.receivers, pf.dump_text());
        assert_eq!(s.receivers.len(), 1, "{:?}\n{}", s.receivers, pf.dump_text());
    }
}

#[test]
fn an_array_copy_is_born_at_its_call_site() {
    // `slice`, `concat` and `filter` return a new array (births per site):
    // a write into the copy does not reach the source. Returning the
    // receiver made commander's `actionArgs = args.slice(..)` then
    // `actionArgs[n] = this.opts()` pour every option value into `args`.
    let pf = facts(
        "class A { a = 1; w = 1; }
        class B { b = 2; w = 2; }
        const s1 = [new A()]; const c1 = s1.slice(); c1.push(new B());
        const s2 = [new A()]; const c2 = s2.concat([new B()]);
        const s3 = [new A()]; const c3 = s3.filter((x) => !!x); c3[1] = new B();
        console.log(s1[0].a, s2[0].a, s3[0].a, c1[0].w, c2.length, c3.length);",
    );
    exact(&pf, "a", 1);
    // The copy holds the source's elements and its own.
    exact(&pf, "w", 2);
}

#[test]
fn an_escaped_value_binds_no_layout_instance() {
    // Unknown code calls the functions an escaped value reaches in their
    // unknown-code instance (`this` = ⊤), one per function: never in the
    // instance of the escaped value's layout. Was: every function of the
    // value ran with `this` = the value per layout (commander: `opts()`'s
    // result holds every Command method; each ran on it, its fields made
    // function receivers, functions x methods instances).
    let src = |esc: &str| format!(
        "class C {{ a() {{ return this.b(); }} b() {{ return 1; }} c() {{ return 2; }} }}
        const o = new C(); console.log(o.a());
        const r: any = {{}};
        for (const k of [\"a\", \"b\", \"c\"]) r[k] = (o as any)[k];
        console.log(r.length);
        {esc}"
    );
    let base = facts(&src("")).stats.instances;
    let esc = facts(&src("(globalThis as any).out = r;")).stats.instances;
    assert!(esc > base, "the value must escape: {base} -> {esc}");
    assert!(esc - base <= 3, "{base} -> {esc}: more than one entry instance per function");
}
