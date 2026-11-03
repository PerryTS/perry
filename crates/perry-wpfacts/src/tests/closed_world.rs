//! Closed-world sets: a points-to set of program objects is never
//! "anything", however many members it has. A site over a large set is
//! judged by layout, and no member escapes because of the set's size.

use super::*;

/// `n` classes, each with the fields `fields(i)`. One binding is assigned
/// an instance of each (its set holds `n` members), and a helper is called
/// with each (its parameter observes `n` members).
fn many(n: usize, fields: impl Fn(usize) -> &'static str) -> ProgramFacts {
    let mut src = String::new();
    for i in 0..n {
        src.push_str(&format!("class C{i} {{ {} }}\n", fields(i)));
    }
    src.push_str("function get(o: any) { return o.x; }\n");
    src.push_str("let v: any = new C0();\n");
    for i in 0..n {
        src.push_str(&format!(
            "if (Math.random() < 0.5) v = new C{i}();\nget(new C{i}());\n"
        ));
    }
    src.push_str("console.log(v.x);\n");
    facts(&src)
}

fn x_sites(pf: &ProgramFacts) -> Vec<&crate::SiteFacts> {
    let v: Vec<_> = pf.sites.iter().filter(|s| s.key == "x").collect();
    assert_eq!(v.len(), 2, "{}", pf.dump_text());
    v
}

fn none_escape(pf: &ProgramFacts, n: usize) {
    for i in 0..n {
        let t = tmpl(pf, &format!("C{i}"));
        assert!(
            t.escape.is_none(),
            "C{i} escapes: {:?}\n{}",
            t.escape,
            pf.dump_text()
        );
    }
}

#[test]
fn a_set_past_64_of_one_layout_is_proven() {
    let n = 80;
    let pf = many(n, |_| "x = 1;");
    for s in x_sites(&pf) {
        assert!(
            !s.top && s.receivers.len() == n,
            "{:?} top={}",
            s.receivers,
            s.top
        );
        assert!(
            matches!(s.verdict, Verdict::T0Mono | Verdict::T0Set),
            "{:?}\n{}",
            s.verdict,
            pf.dump_text()
        );
    }
    none_escape(&pf, n);
    assert!(!pf.dump_text().contains("widen"), "{}", pf.dump_text());
}

#[test]
fn a_set_past_64_of_two_slots_differs_and_nothing_escapes() {
    let n = 80;
    let pf = many(n, |i| {
        if i % 2 == 0 {
            "x = 1;"
        } else {
            "y = 2; x = 1;"
        }
    });
    for s in x_sites(&pf) {
        assert!(
            !s.top && s.receivers.len() == n,
            "{:?} top={}",
            s.receivers,
            s.top
        );
        assert_eq!(
            s.verdict,
            Verdict::No("slots differ across the receiver set".into()),
            "{}",
            pf.dump_text()
        );
    }
    none_escape(&pf, n);
    assert!(!pf.dump_text().contains("widen"), "{}", pf.dump_text());
}
