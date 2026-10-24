//! Object tokens per birth site: `new C()` at each site is its own token,
//! and a summarized function sees its own call's `this`. Layout facts stay
//! per template: many site tokens of one class are one receiver layout.

use super::*;

/// Two vectors filled with different element classes. Each vector's
/// storage is born in its own birth run, and `append` runs per receiver
/// object, so each storage holds its own elements (CWL had one `append`
/// instance per layout writing both storages).
#[test]
fn two_vector_objects_of_one_layout_keep_their_appends() {
    let pf = facts(
        r#"
        class A { a = 1; }
        class B { b = "x"; }
        class Vec {
            storage: any[];
            n: number;
            constructor() { this.storage = new Array(10); this.n = 0; }
            append(e: any) { this.storage[this.n] = e; this.n++; }
            at(i: number) { return this.storage[i]; }
        }
        const va = new Vec();
        va.append(new A());
        const vb = new Vec();
        vb.append(new B());
        console.log(va.at(0).a, vb.at(0).b);
        "#,
    );
    for (key, want) in [("a", "t.ts:A"), ("b", "t.ts:B")] {
        let site = pf
            .sites
            .iter()
            .find(|s| s.key == key && !s.is_call)
            .expect("site");
        assert_eq!(site.receivers, vec![want.to_string()], "{}", pf.dump_text());
    }
    let t = tmpl(&pf, "Vec");
    assert!(t.escape.is_none() && t.hard.is_none(), "{}", pf.dump_text());
}

/// One class born at many sites and read through one helper: one layout,
/// proven, and the template keeps its facts.
#[test]
fn many_sites_of_one_class_are_one_layout() {
    let mut src = String::from("class P { x = 1; y = 2; }\nfunction get(o: any) { return o.x; }\n");
    for _ in 0..40 {
        src.push_str("get(new P());\n");
    }
    let pf = facts(&src);
    let site = pf.sites.iter().find(|s| s.key == "x").expect("site");
    assert_eq!(
        site.receivers,
        vec!["t.ts:P".to_string()],
        "{}",
        pf.dump_text()
    );
    assert_eq!(site.verdict, Verdict::T0Mono, "{}", pf.dump_text());
    let t = tmpl(&pf, "P");
    assert!(t.escape.is_none() && t.hard.is_none(), "{}", pf.dump_text());
}

/// A template-wide mark (a dynamic `extends`) holds for every birth-site
/// token, including those born after the mark.
#[test]
fn a_template_wide_mark_holds_for_every_site() {
    let pf = facts(
        r#"
        declare const Base: any;
        class D extends Base { x = 1; }
        function get(o: any) { return o.x; }
        get(new D());
        get(new D());
        "#,
    );
    let t = tmpl(&pf, "D");
    assert!(t.escape.is_some() || t.hard.is_some(), "{}", pf.dump_text());
    let site = pf.sites.iter().find(|s| s.key == "x").expect("site");
    assert!(matches!(site.verdict, Verdict::No(_)), "{}", pf.dump_text());
}

/// A subclass constructor (`super(...)`) is summarized: each `new` runs the
/// parent constructor on its own object, and `append` runs per object, so
/// the two storages stay apart.
#[test]
fn subclass_objects_of_one_layout_keep_their_appends() {
    let pf = facts(
        r#"
        class A { a = 1; }
        class B { b = "x"; }
        class Vec {
            storage: any[];
            n: number;
            constructor() { this.storage = new Array(10); this.n = 0; }
            append(e: any) { this.storage[this.n] = e; this.n++; }
            at(i: number) { return this.storage[i]; }
        }
        class Plan extends Vec { constructor() { super(); } }
        const pa = new Plan();
        pa.append(new A());
        const pb = new Plan();
        pb.append(new B());
        console.log(pa.at(0).a, pb.at(0).b);
        "#,
    );
    for (key, want) in [("a", "t.ts:A"), ("b", "t.ts:B")] {
        let site = pf
            .sites
            .iter()
            .find(|s| s.key == key && !s.is_call)
            .expect("site");
        assert_eq!(site.receivers, vec![want.to_string()], "{}", pf.dump_text());
    }
}
