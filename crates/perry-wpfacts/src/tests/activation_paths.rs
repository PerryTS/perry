//! DECISIONS 225: each activation path is the only reason some body runs.
//! A program whose marker function `hit` runs only through the body under
//! test: `hit` is active, and with the paths that carry that body left out
//! (`PERRY_FACTS_SABOTAGE=activation:<path>[+<path>..]`) it is not, so a
//! claim that dropped those paths would exclude a body Node runs. Two paths
//! carry one transfer where the solver activates twice: a plain call into a
//! summarized callee (`call` and its `this` binding), a getter under a
//! constant computed key (`computed` at registration and `accessor` at the
//! read). Leaving out either alone keeps the body active (a control).
//! A private method called from the constructor of a class a mixin extends
//! is the first kind: its call selects the declaring member (CXCHK0 found
//!  excluded while Node ran it, before private calls were followed).
//! `instance`, `composed` and `for-in` fire only inside a transfer that
//! `call`, `accessor`, `this` or `for-in`'s caller already activates (an
//! instance is made by a call binding; a composed callee's calls are
//! re-solved as calls of the instance), so no program reaches a body
//! through them alone.

use super::*;

/// Whether `hit` can run in `src` with the activation paths `sab` left out.
fn hit_runs(src: &str, sab: Option<&str>) -> bool {
    let module = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let cfg = FactsConfig {
        sabotage: sab.map(|s| format!("activation:{s}")),
        max_work: Some(50_000_000),
        ..FactsConfig::default()
    };
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], &cfg);
    let on = pf.active_bodies.as_ref().expect("activated");
    let id = hir
        .functions
        .iter()
        .find(|f| f.name == "hit")
        .expect("hit")
        .id;
    on.iter().any(|(_, i, _)| *i == id)
}

/// Each fixture, the path set that alone excludes it, and the single paths
/// (controls) that leave it active.
const FIXTURES: &[(&str, &str, &str, &[&str])] = &[
    (
        "summarized-callee",
        r#"
function hit() { return 1; }
function m(x: any): any { if (x > 100) return m(x - 1); hit(); return x; }
function outer(y: any) { return m(y); }
console.log(outer(1), outer("s"));
"#,
        "call+this",
        &["call", "this", "instance", "composed"],
    ),
    (
        "computed-getter",
        r#"
function hit() { return 1; }
const k = "dyn";
class C { get [k]() { return hit() + 6; } }
console.log((new C() as any).dyn);
"#,
        "computed+accessor",
        &["computed", "accessor", "call"],
    ),
    (
        "private-method-of-a-mixin-base",
        r#"
function hit() { return 1; }
class Client {
  opts: any;
  constructor(o: any) { this.opts = this.#init(o); }
  #init(o: any) { return { ...o, ok: hit() > 0 }; }
}
function attach(Base: any) { return class extends Base {}; }
const Sub = attach(Client);
console.log(new Sub({ a: 1 }).opts.ok);
console.log(new Client({}).opts.ok);
"#,
        "call+this",
        &["call", "this", "escape", "accessor"],
    ),
];

#[test]
fn each_fixture_runs_its_body() {
    for (name, src, _, _) in FIXTURES {
        assert!(hit_runs(src, None), "{name}: hit inactive without sabotage");
    }
}

#[test]
fn leaving_out_the_carrying_paths_excludes_the_body() {
    for (name, src, set, _) in FIXTURES {
        assert!(
            !hit_runs(src, Some(set)),
            "{name}: hit active with {set} left out"
        );
    }
}

#[test]
fn leaving_out_one_carrier_keeps_the_body() {
    for (name, src, _, controls) in FIXTURES {
        for p in *controls {
            assert!(
                hit_runs(src, Some(p)),
                "{name}: hit inactive with only {p} left out"
            );
        }
    }
}
