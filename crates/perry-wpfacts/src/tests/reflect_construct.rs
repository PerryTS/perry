//! Reflect construction runs the target's body on the new target's allocation.
use super::*;

fn receivers(pf: &ProgramFacts, key: &str, want: &[&str]) {
    let sites: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    assert!(!sites.is_empty(), "{key}: {}", pf.dump_text());
    for s in sites {
        let mut got: Vec<_> = s
            .receivers
            .iter()
            .map(|r| r.rsplit(':').next().unwrap())
            .collect();
        got.sort();
        assert!(!s.top && got == want, "{key}: {s:?}\n{}", pf.dump_text());
    }
}

#[test]
fn reflect_construct_result_names_the_allocation_class() {
    let pf = facts(
        r#"
        class Two { r = 1; }
        class Other { pad = 99; r = 7; }
        const value: any = Reflect.construct(Two, [], Other);
        console.log(value.r);
    "#,
    );
    receivers(&pf, "r", &["Other"]);
}

#[test]
fn reflect_construct_target_body_uses_the_allocation_receiver() {
    let pf = facts(
        r#"
        class Two {
            body = 1;
            constructor() { console.log(this.body); }
        }
        class Other { pad = 99; body = 7; }
        Reflect.construct(Two, [], Other);
    "#,
    );
    receivers(&pf, "body", &["Other"]);
}

#[test]
fn reflect_construct_bound_target_keeps_new_target_for_body_and_result() {
    let pf = facts(
        r#"
        function Two(this: any) { this.r = 1; this.body = 2; console.log(this.body); }
        class Other { pad = 99; r = 7; body = 8; }
        const target: any = Two.bind(null);
        const value: any = Reflect.construct(target, [], Other);
        console.log(value.r);
    "#,
    );
    receivers(&pf, "r", &["Other"]);
    receivers(&pf, "body", &["Other"]);
}

#[test]
fn reflect_construct_same_or_default_new_target_keeps_the_target_class() {
    for call in [
        "Reflect.construct(Two, [], Two)",
        "Reflect.construct(Two, [])",
    ] {
        let pf = facts(&format!(
            r#"
            class Two {{ r = 1; body = 2; constructor() {{ console.log(this.body); }} }}
            const value: any = {call}; console.log(value.r);
        "#
        ));
        receivers(&pf, "r", &["Two"]);
        receivers(&pf, "body", &["Two"]);
    }
}

#[test]
fn reflect_construct_keeps_object_returns_beside_the_actual_allocation() {
    let pf = facts(
        r#"
        class Replacement { r = 9; }
        class Two { body = 1; constructor() { console.log(this.body); return new Replacement() as any; } }
        class Other { pad = 99; body = 7; }
        const value: any = Reflect.construct(Two, [], Other);
        console.log(value.r);
    "#,
    );
    receivers(&pf, "r", &["Other", "Replacement"]);
    receivers(&pf, "body", &["Other"]);
}

#[test]
fn reflect_construct_unknown_new_target_does_not_invent_target_instances() {
    let pf = facts(
        r#"
        declare function opaque(): any;
        class Two { r = 1; body = 2; constructor() { console.log(this.body); } }
        const value: any = Reflect.construct(Two, [], opaque());
        console.log(value.r);
    "#,
    );
    for key in ["r", "body"] {
        let s = pf.sites.iter().find(|s| s.key == key).expect("read");
        assert!(s.top, "{s:?}\n{}", pf.dump_text());
        assert!(matches!(s.verdict, Verdict::No(_)), "{s:?}");
    }
}

#[test]
fn reflect_construct_unknown_target_keeps_the_result_unknown() {
    let pf = facts(
        r#"
        declare function opaque(): any;
        class Other { pad = 99; r = 7; }
        const value: any = Reflect.construct(opaque(), [], Other);
        console.log(value.r);
    "#,
    );
    let s = pf.sites.iter().find(|s| s.key == "r").expect("read");
    assert!(s.top && s.receivers.is_empty(), "{s:?}\n{}", pf.dump_text());
}

#[test]
fn reflect_construct_does_not_erase_ordinary_target_facts() {
    let pf = facts(
        r#"
        class Two { ordinary = 1; }
        class Other { pad = 99; }
        Reflect.construct(Two, [], Other);
        console.log(new Two().ordinary);
    "#,
    );
    receivers(&pf, "ordinary", &["Two"]);
    let s = pf.sites.iter().find(|s| s.key == "ordinary").unwrap();
    assert!(
        matches!(s.verdict, Verdict::T0Mono),
        "{s:?}\n{}",
        pf.dump_text()
    );
}

#[test]
fn reflect_construct_new_target_expression_never_supplies_the_body_receiver() {
    let pf = facts(
        r#"
        class Two { r = 1; body = 2; constructor() { console.log((new.target as any).tag, this.body); } }
        class Other { static tag = 7; pad = 99; r = 7; body = 8; }
        const value: any = Reflect.construct(Two, [], Other); console.log(value.r);
    "#,
    );
    receivers(&pf, "r", &["Other"]);
    receivers(&pf, "body", &["Other"]);
}

#[test]
fn an_escaped_class_constructor_accepts_unknown_new_target_receivers() {
    let pf = facts(
        r#"
        class Two { body = 1; constructor() { console.log(this.body); } }
        class Other { pad = 99; body = 7; }
        (globalThis as any).target = Two;
        const target: any = Reflect.get(globalThis, ["tar", "get"].join(""));
        Reflect.construct(target, [], Other);
    "#,
    );
    let s = pf
        .sites
        .iter()
        .find(|s| s.key == "body")
        .expect("body read");
    assert!(
        s.receivers.iter().any(|r| r.ends_with(":Other")),
        "{s:?}\n{}",
        pf.dump_text()
    );
    assert!(!s.top, "the actual allocation is known: {s:?}");
}

#[test]
fn reflect_construct_helper_keeps_each_calls_allocation_receiver() {
    let pf = facts(
        r#"
        function make(target: any, nt: any) { return Reflect.construct(target, [], nt); }
        class A { a = 1; constructor() { console.log(this.a); } }
        class B { b = 2; constructor() { console.log(this.b); } }
        class P { pad = 9; a = 7; }
        class Q { pad = 9; b = 7; }
        const p: any = make(A, P); const q: any = make(B, Q);
        console.log(p.a, q.b);
    "#,
    );
    receivers(&pf, "a", &["P"]);
    receivers(&pf, "b", &["Q"]);
}

#[test]
fn escaping_a_class_without_reflect_construction_adds_no_arbitrary_receiver() {
    // Unseen code may construct the escaped class.
    let pf = facts(&unseen(
        r#"
        class Two { body = 1; constructor() { console.log(this.body); } }
        (globalThis as any).target = Two;
    "#,
    ));
    receivers(&pf, "body", &["Two"]);
}

#[test]
fn reflect_unknown_target_reaches_a_constructor_that_escapes_later() {
    // `opaque` is an unseen module's function; `target` is where it may read.
    let pf = facts(&unseen(
        r#"
        declare function opaque(): any;
        class Two { body = 1; constructor() { console.log(this.body); } }
        class Other { pad = 99; body = 7; }
        const value: any = Reflect.construct(opaque(), [], Other);
        if (value) (globalThis as any).target = Two;
    "#,
    ));
    let s = pf
        .sites
        .iter()
        .find(|s| s.key == "body")
        .expect("body read");
    assert!(
        s.receivers.iter().any(|r| r.ends_with(":Other")),
        "{s:?}\n{}",
        pf.dump_text()
    );
}
