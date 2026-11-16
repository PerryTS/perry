//! ECMA-262 27.2.4.1 Promise.all: every evaluation runs PerformPromiseAll,
//! which makes a new values Array (ArrayCreate, 27.2.4.1.2 step 1), so the
//! result arrays of two calls never share elements. The solver births the
//! result per (call site, the calling run's maker context)
//! (`builtins::site_tmpl_born` -> `Model::birth_ctx` -> `made_ctx`), the
//! context a literal of the same run gets (`fill_instance` births): two
//! runs of `dep` with their own maker context get two result arrays.
//! (three.js GLTFLoader `getDependencies(type)`: `Promise.all(defs.map(...))`,
//! read as `dependencies[0][json.scene || 0]`.)
use super::*;

fn src(calls: &str) -> String {
    format!(
        r#"
        class S {{ f = 1; }}
        class A {{ g = 2; }}
        function mkS(): any[] {{ return [new S()]; }}
        function mkA(): any[] {{ return [new A()]; }}
        function dep(xs: any[]): any {{ return Promise.all(xs); }}
        const unknown: any = (globalThis as any)[process.argv[2]];
        const k: any = Number(process.argv[3]);
        {calls}
            .then((d: any) => {{ const s = d[0][k]; unknown.take(d[1]); console.log(s.f); }});
        "#
    )
}

/// Both lanes run with the per-index rule of the outer literal
/// `Promise.all([...])` on (`promise_all_positions`); without it `d[0]` is
/// every item of the outer literal and the fixture shows nothing.
fn run(calls: &str) -> ProgramFacts {
    std::env::set_var("PERRY_FACTS_PROMISE_ALL_INDEX", "1");
    let pf = facts(&src(calls));
    std::env::remove_var("PERRY_FACTS_PROMISE_ALL_INDEX");
    pf
}

fn escapes(pf: &ProgramFacts, name: &str) -> bool {
    tmpl(pf, name).escape.is_some()
}

/// Two calls of `dep`: two result arrays; `d[1]`'s A escapes, `d[0]`'s S
/// never reaches the sink.
#[test]
fn each_call_has_its_own_result_array() {
    let pf = run(r#"Promise.all([dep(mkS()), dep(mkA())])"#);
    assert!(escapes(&pf, "A"), "A kept\n{}", pf.dump_text());
    assert!(!escapes(&pf, "S"), "S escaped\n{}", pf.dump_text());
}

/// Control: one result array in both positions: S reaches the sink.
#[test]
fn one_result_array_in_both_positions_escapes_s() {
    let pf = run(r#"const one: any = dep(mkS()); Promise.all([one, one])"#);
    assert!(escapes(&pf, "S"), "S kept\n{}", pf.dump_text());
}

/// The GLTFLoader shape: `dep` a method of one parser object, called from
/// a callback. Every call binds the run of that receiver (`bind_layout_run`
/// key kind 1: the receiver's maker key), one run for all three types.
#[test]
fn method_calls_on_one_receiver() {
    let pf = run(
        r#"class P { dep(xs: any[]): any { return Promise.all(xs); } }
        const p = new P();
        Promise.resolve(0).then(() => Promise.all([p.dep(mkS()), p.dep(mkA())]))"#,
    );
    eprintln!("OVMB57 method S escapes={} A escapes={}", escapes(&pf, "S"), escapes(&pf, "A"));
}

#[test]
fn ovmb57_probe_variants() {
    for (name, calls) in [
        ("k0", r#"Promise.all([dep(mkS()), dep(mkA())]).then((d: any) => { const s = d[0][0]; unknown.take(d[1]); console.log(s.f); }); Promise.resolve(0)"#),
        ("noTake", r#"Promise.all([dep(mkS()), dep(mkA())]).then((d: any) => { const s = d[0][k]; console.log(s.f); }); Promise.resolve(0)"#),
        ("direct", r#"Promise.all([Promise.all(mkS()), Promise.all(mkA())])"#),
    ] {
        let pf = run(calls);
        let f = pf.sites.iter().filter(|s| s.key == "f").map(|s| format!("{:?}", s.verdict)).collect::<Vec<_>>();
        eprintln!("OVMB57 {name} S={} A={} f={f:?}", escapes(&pf, "S"), escapes(&pf, "A"));
    }
}
