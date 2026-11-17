use super::*;

const SRC: &str = "function dead() { return 1; }\n\
function live() { return 2; }\n\
function alsoDead() { return dead(); }\n\
console.log(live());\n";

fn id_of(src: &str, name: &str) -> u32 {
    let m = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&m, "t", "t.ts").expect("lowers");
    hir.functions
        .iter()
        .find(|f| f.name == name)
        .expect(name)
        .id
}

fn ident(id: u32) -> crate::BodyIdentity {
    ("t.ts".to_string(), id, String::new())
}

#[test]
fn inactive_is_the_complement_of_reach_for_classified_bodies() {
    let pf = facts(SRC);
    let (dead, live) = (id_of(SRC, "dead"), id_of(SRC, "live"));
    assert!(pf.body_reach.complete);
    assert!(pf.body_reach.inactive(&ident(dead)), "dead can never run");
    assert!(!pf.body_reach.inactive(&ident(live)), "live runs");
    // A new or unknown identity is active, never inactive.
    assert!(!pf.body_reach.inactive(&ident(9_999)));
    assert!(!pf
        .body_reach
        .inactive(&("other.ts".to_string(), dead, String::new())));
}

#[test]
fn an_incomplete_reach_marks_nothing_inactive() {
    // Enough work to exercise the hard stop (as the export's own test).
    let big = format!(
        "{SRC}function f(x: any) {{ return x.k; }} {}",
        "console.log(f({k: 1}));".repeat(256)
    );
    let pf = facts_with(
        &big,
        FactsConfig {
            max_work: Some(1),
            ..Default::default()
        },
    );
    assert!(
        pf.stats.aborted && !pf.body_reach.complete,
        "the control must be interrupted"
    );
    let dead = id_of(&big, "dead");
    assert!(!pf.body_reach.inactive(&ident(dead)));
    // The same set, completed, calls it inactive: the flag is what stops it.
    let mut done = facts(&big).body_reach;
    assert!(done.inactive(&ident(dead)));
    done.complete = false;
    assert!(!done.inactive(&ident(dead)));
}

#[test]
fn the_exported_classification_is_the_reach_predicate() {
    let pf = facts_with(
        SRC,
        FactsConfig {
            dce_bodies: true,
            ..Default::default()
        },
    );
    let map = pf.verification_bodies.as_ref().expect("exported");
    assert!(!map.is_empty());
    for (k, active) in map {
        assert_eq!(*active, !pf.body_reach.inactive(k), "{k:?}");
    }
    assert_eq!(map.get(&ident(id_of(SRC, "dead"))), Some(&false));
}

#[test]
fn an_async_generator_return_reads_the_promise_constructor() {
    // AsyncGenerator.prototype.return runs PromiseResolve(%Promise%, value),
    // which calls a `constructor` getter on a promise argument; `mark` runs
    // exactly when the getter does.
    let src = "async function* rows() { yield 1; }\n\
function mark() { return 0; }\n\
const value = Promise.resolve(3);\n\
Object.defineProperty(value, \"constructor\", { get() { mark(); return Promise; } });\n\
rows().return(value).then((r) => console.log(r.done));\n";
    let ast = perry_parser::parse_typescript(src, "t.ts").expect("parses");
    let mut hir = perry_hir::lower_module(&ast, "t", "t.ts").expect("lowers");
    perry_transform::transform_generators(&mut hir);
    let mark = hir.functions.iter().find(|f| f.name == "mark").expect("mark").id;
    let pf = crate::compute(&[("t.ts".to_string(), &hir)], &FactsConfig::default());
    assert!(pf.body_reach.complete);
    assert!(!pf.body_reach.inactive(&ident(mark)), "the getter runs, so mark does");
}

#[test]
fn a_proxy_call_method_runs_the_apply_trap() {
    // `p.call(...)` on a Proxy is the HIR's fused `ProxyGet(p, "call")` call:
    // the runtime runs the Proxy's [[Call]], so the handler's `apply` trap
    // runs; `mark` runs exactly when the trap does.
    let src = "function mark() { return 0; }\n\
function method(this: any, a: number) { return this.x + a; }\n\
const callable: any = new Proxy(method, { apply(t: any, r: any, args: any[]) { mark(); return t.apply(r, args); } });\n\
console.log(callable.call({ x: 7 }, 2));\n";
    let pf = facts(src);
    assert!(pf.body_reach.complete);
    assert!(!pf.body_reach.inactive(&ident(id_of(src, "mark"))), "the apply trap runs");
    assert!(!pf.body_reach.inactive(&ident(id_of(src, "method"))), "the target runs");
}
