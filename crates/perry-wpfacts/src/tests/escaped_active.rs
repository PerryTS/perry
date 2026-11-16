//! DECISIONS 225: every function the final solve treats as escaped
//! (callable by unknown code) was walked by the activation pass. On
//! d437d8eff3 the activation pass gave every escaped function the
//! own-return rule (it has no summaries) while the final solve escapes a
//! summarized function through its escape instance: three.js skin escaped
//! AnimationClip's statics and methods in the final solve with their bodies
//! unwalked (9 functions, 81 sites).

use super::*;

/// The escaped functions' bodies all ran in the activation pass, and the
/// solve says so (`# escaped-inactive=0`).
fn covered(pf: &ProgramFacts) {
    assert_eq!(pf.escaped_inactive, Some(0), "{}", pf.dump_text());
    assert!(
        pf.dump_text().contains("# escaped-inactive=0\n"),
        "{}",
        pf.dump_text()
    );
}

const ESCAPED: &str = r#"
class Clip {
    tracks: any;
    constructor(t: any) { this.tracks = t; }
    static parse(json: any) { return new Clip(json.tracks); }
    trim() { const t = this.tracks; return t; }
}
function keep(x: any) { return x; }
function load(i: number) { return Promise.resolve(keep(new Clip([i]))); }
const all = Promise.all([0, 1].map(function (i) { return load(i); }));
(globalThis as any).keep = keep;
all.then(function (cs: any) { (globalThis as any).out = cs; });
(globalThis as any).Clip = Clip;
"#;

#[test]
fn an_escaped_class_has_its_static_and_method_bodies_walked() {
    let pf = facts(ESCAPED);
    covered(&pf);
}

#[test]
fn an_uncalled_unescaped_function_stays_inactive() {
    // Control: the invariant does not activate everything.
    let pf = facts(
        "function never(o: any) { return o.zz; }\nfunction id(x: any) { return x; }\n(globalThis as any).id = id;\nconsole.log(id(1));\n",
    );
    covered(&pf);
    assert!(pf.sites.iter().all(|s| s.key != "zz"), "{}", pf.dump_text());
}
