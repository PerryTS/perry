//! A parameter that the body only calls (never assigned, stored, passed on
//! or returned; no `arguments`, no direct eval) makes the function's result
//! depend on which function value the call passes. A call whose argument
//! holds exactly one function value runs the function once per that value
//! (three.js `GLTFParser._invokeOne(func)`, which only calls `func(ext)`):
//! each call's result is what its own function value returns (ECMA-262
//! 10.2.1 [[Call]]: the parameter is bound to the argument of that call).
//! An argument of several or unknown function values, a parameter that is
//! stored or reassigned, keeps the shared run.
use super::*;

fn src(inv: &str, args: [&str; 3]) -> String {
    format!(
        "class A {{ p = 1; }}
         class B {{ p = 2; }}
         class C {{ p = 3; }}
         class H {{ h: any = null; }}
         const holder: any = new H();
         function call(g: any) {{ return g(); }}
         class P {{
           {inv}
           go() {{
             const a: any = this.inv({});
             const b: any = this.inv({});
             const c: any = this.inv({});
             console.log(a.p, b.p, c.p);
           }}
         }}
         new P().go();",
        args[0], args[1], args[2]
    )
}

const ARGS: [&str; 3] = ["() => new A()", "() => new B()", "() => new C()"];

/// The classes the program's `.p` reads see.
fn per_read(pf: &ProgramFacts) -> Vec<Vec<String>> {
    let mut v: Vec<Vec<String>> = pf
        .sites
        .iter()
        .filter(|s| s.key == "p" && !s.is_call)
        .map(|s| {
            let mut r: Vec<String> = s
                .receivers
                .iter()
                .map(|r| r.rsplit(':').next().unwrap_or(r).to_string())
                .collect();
            r.sort();
            r.dedup();
            r
        })
        .collect();
    v.sort();
    v
}

fn split(v: &[Vec<String>]) -> bool {
    v.iter().all(|r| r.len() == 1)
}

#[test]
fn called_parameter_runs_per_function_value() {
    let pf = facts(&src("inv(f: any) { return f(this); }", ARGS));
    let v = per_read(&pf);
    assert_eq!(v, vec![vec!["A"], vec!["B"], vec!["C"]]);
}

#[test]
fn called_parameter_with_an_argument_runs_per_function_value() {
    let pf = facts(&src(
        "inv(f: any) { for (const x of [1, 2]) { const r = f(x); if (r) return r; } return null; }",
        ARGS,
    ));
    assert!(split(&per_read(&pf)), "{:?}", per_read(&pf));
}

#[test]
fn argument_of_two_function_values_keeps_the_shared_run() {
    let pf = facts(&src(
        "inv(f: any) { return f(this); }",
        ["process.argv[2] ? (() => new A()) : (() => new B())", "() => new B()", "() => new C()"],
    ));
    let v = per_read(&pf);
    assert!(v.iter().any(|r| r.contains(&"A".to_string()) && r.contains(&"B".to_string())), "{v:?}");
}

#[test]
fn stored_parameter_keeps_the_shared_run() {
    let pf = facts(&src("inv(f: any) { holder.h = f; return f(); }", ARGS));
    assert!(!split(&per_read(&pf)), "{:?}", per_read(&pf));
}

#[test]
fn reassigned_parameter_keeps_the_shared_run() {
    let pf = facts(&src(
        "inv(f: any) { if (process.argv[3]) f = () => new C(); return f(); }",
        ARGS,
    ));
    assert!(!split(&per_read(&pf)), "{:?}", per_read(&pf));
}

#[test]
fn passed_on_parameter_keeps_the_shared_run() {
    let pf = facts(&src(
        "inv(f: any) { return call(f); }",
        ARGS,
    ));
    // `call`'s own parameter is only called: one level of value runs is
    // the rule's reach; `inv` passes `f` on, so its runs are shared.
    let v = per_read(&pf);
    assert!(!split(&v), "{v:?}");
}
