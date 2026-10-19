//! Captured parameters and locals are summary terms: a closure created in
//! an instance carries that instance's nodes (its context), so callers
//! never merge through what a closure captured.

use super::*;

const CLASSES: &str = r#"
    class A { x = 1; a = 1; }
    class B { y = 2; b = 2; }
"#;

fn sites<'a>(pf: &'a ProgramFacts, key: &str) -> Vec<&'a crate::SiteFacts> {
    let v: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    assert!(!v.is_empty(), "site {key} in\n{}", pf.dump_text());
    v
}

/// Every read of `key` sees exactly `n` receivers and no ⊤.
fn exact(pf: &ProgramFacts, key: &str, n: usize) {
    for s in sites(pf, key) {
        assert!(!s.top && s.receivers.len() == n, "{key}: {:?} top={}\n{}", s.receivers, s.top, pf.dump_text());
    }
}

/// Every read of `key` sees `class` among its receivers (or ⊤).
fn sees(pf: &ProgramFacts, key: &str, class: &str) {
    for s in sites(pf, key) {
        let hit = s.receivers.iter().any(|r| r.ends_with(&format!(":{class}")));
        assert!(s.top || hit, "{key}: {:?} lacks {class}\n{}", s.receivers, pf.dump_text());
    }
}

/// No read of `key` sees `class`.
fn lacks(pf: &ProgramFacts, key: &str, class: &str) {
    for s in sites(pf, key) {
        let hit = s.receivers.iter().any(|r| r.ends_with(&format!(":{class}")));
        assert!(!s.top && !hit, "{key}: {:?} top={} has {class}\n{}", s.receivers, s.top, pf.dump_text());
    }
}

#[test]
fn closure_over_a_parameter_keeps_callers_apart() {
    let pf = facts(&format!(
        "{CLASSES}
        function f(x: any) {{ return [1].map(() => x); }}
        const a = f(new A()); const b = f(new B());
        console.log(a[0].x, b[0].y);"
    ));
    exact(&pf, "x", 1);
    exact(&pf, "y", 1);
}

#[test]
fn arrow_this_is_the_enclosing_instance_this() {
    let pf = facts(&format!(
        "{CLASSES}
        class H {{ o: any; constructor(o: any) {{ this.o = o; }} run() {{ return [1].map(() => this.o); }} }}
        const a = new H(new A()).run(); const b = new H(new B()).run();
        console.log(a[0].x, b[0].y);"
    ));
    exact(&pf, "x", 1);
    exact(&pf, "y", 1);
}

/// deltablue: `satisfy(mark)` marks its inputs through a closure; a call
/// passing an object elsewhere never reaches the other constraint's inputs.
#[test]
fn deltablue_mark_stays_primitive() {
    let pf = facts(
        "class V { mark: any = 0; }
         class W { mark: any = 0; }
         class O { k = 1; }
         class C {
           v: any;
           constructor(v: any) { this.v = v; }
           inputsDo(fn: any) { fn(this.v); }
           satisfy(mark: any) { this.inputsDo((i: any) => { i.mark = mark; }); }
         }
         const v = new V();
         new C(v).satisfy(1);
         new C(new W()).satisfy(new O());
         console.log(v.mark.k);",
    );
    lacks(&pf, "k", "O");
}

#[test]
fn nested_closures_reach_the_outer_instance() {
    let pf = facts(&format!(
        "{CLASSES}
        function f(x: any) {{ return () => () => x; }}
        const a = f(new A())()(); const b = f(new B())()();
        console.log(a.x, b.y);"
    ));
    exact(&pf, "x", 1);
    exact(&pf, "y", 1);
}

#[test]
fn captured_local_written_after_capture() {
    let pf = facts(&format!(
        "{CLASSES}
        function f(o: any) {{ let v: any = 0; const set = () => {{ v = o; }}; set(); return v; }}
        const a = f(new A()); const b = f(new B());
        console.log(a.x, b.y);"
    ));
    exact(&pf, "x", 1);
    exact(&pf, "y", 1);
}

/// A closure kept past its call, called from module code: every context
/// it was made in reaches the call.
#[test]
fn closure_outliving_its_call_sees_every_context() {
    let pf = facts(&format!(
        "{CLASSES}
        let saved: any;
        function f(o: any) {{ saved = () => o; }}
        f(new A()); f(new B());
        console.log(saved().x);"
    ));
    sees(&pf, "x", "A");
    sees(&pf, "x", "B");
}

/// Unknown code calls an escaped closure: what it writes into the captured
/// local is unknown, in the instance that made the closure.
#[test]
fn sabotage_escaped_closure_writes_its_capture() {
    let pf = facts(&format!(
        "{CLASSES}
        declare const ext: any;
        function f(o: any) {{ let v: any = o; ext((n: any) => {{ v = n; }}); return v; }}
        const r = f(new A());
        console.log(r.x);"
    ));
    let x = &sites(&pf, "x")[0];
    assert!(x.top, "{:?}\n{}", x.receivers, pf.dump_text());
}

/// A closure stored on an escaped object is called by unknown code with
/// unknown arguments, and its result is unknown to its reader.
#[test]
fn sabotage_closure_through_an_escaped_field() {
    let pf = facts(&format!(
        "{CLASSES}
        declare const ext: any;
        function f(o: any) {{ const h: any = {{}}; h.g = (n: any) => {{ o.v = n; }}; ext(h); return o; }}
        const r = f(new A());
        console.log(r.v.q);"
    ));
    let q = &sites(&pf, "q")[0];
    assert!(q.top, "{:?}\n{}", q.receivers, pf.dump_text());
}
