//! A summary run keyed by a string value (rule M1: a switch on the
//! parameter; rule M3: a parameter used only as a key) is its own lexical
//! context (ECMA-262 10.2.1.1: each call has its own environment; what it
//! makes is that environment's): the objects, closures and builtin results
//! born in the run of 'a' are other tokens than those born in the run of
//! 'b'. Two calls with the same constant share one run, and a string of
//! unknown value runs the one merged run.
use super::*;

fn recv(pf: &ProgramFacts) -> Vec<String> {
    let mut r: Vec<String> = pf
        .sites
        .iter()
        .filter(|s| s.key == "p" && !s.is_call)
        .flat_map(|s| s.receivers.iter())
        .map(|r| r.rsplit(':').next().unwrap_or(r).to_string())
        .collect();
    r.sort();
    r.dedup();
    r
}

const SW: &str = "switch (t) { case 'a': break; case 'b': break; }";

/// A method bound per receiver (a plain function's static call is
/// composed per site already): `pre` is the switch (M1) or the key use
/// (M3), `ret` what the run returns, `read` takes the A run's value out of
/// it; the B run is the other call.
fn src(pre: &str, ret: &str, k1: &str, k2: &str, read: &str) -> String {
    format!(
        "class A {{ p = 1; }}
         class B {{ p = 2; }}
         class P {{
           box(t: any, v: any): any {{ {pre} {ret} }}
           go() {{
             const x: any = {read}(this.box({k1}, new A()));
             this.box({k2}, new B());
             console.log(x.p);
           }}
         }}
         new P().go();"
    )
}

/// Receivers of each `.p` read.
fn per_read(pf: &ProgramFacts) -> Vec<Vec<String>> {
    let mut v: Vec<(u32, Vec<String>)> = pf
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
            (s.byte_offset, r)
        })
        .collect();
    v.sort();
    v.into_iter().map(|x| x.1).collect()
}

fn check(pre: &str, ret: &str, read: &str, k1: &str, k2: &str, want: &[&str]) {
    let s = src(pre, ret, k1, k2, read);
    let pf = facts(&s);
    let got = per_read(&pf);
    let w: Vec<String> = want.iter().map(|x| x.to_string()).collect();
    assert!(!got.is_empty(), "no .p read: {s}");
    for r in &got {
        assert_eq!(r, &w, "receivers {got:?} (all .p reads; {})", recv(&pf).join(","));
    }
}

const OBJ: (&str, &str) = ("return { v };", "((o: any) => o.v)");
const CLO: (&str, &str) = ("return () => v;", "((f: any) => f())");
/// The builtin's result array is born in the run (`birth_ctx` through
/// `call_lay`), its callback is a closure the run makes.
const MAP: (&str, &str) = ("return [0].map((e: any) => v);", "((a: any) => a[0])");

#[test]
fn switch_run_literal_is_per_value() {
    check(SW, OBJ.0, OBJ.1, "'a'", "'b'", &["A"]);
}

#[test]
fn switch_run_closure_is_per_value() {
    check(SW, CLO.0, CLO.1, "'a'", "'b'", &["A"]);
}

#[test]
fn switch_run_builtin_result_is_per_value() {
    check(SW, MAP.0, MAP.1, "'a'", "'b'", &["A"]);
}

const KEYED: &str = "const o: any = {}; o[t] = 1;";

#[test]
fn keyed_run_literal_is_per_value() {
    check(KEYED, OBJ.0, OBJ.1, "'a'", "'b'", &["A"]);
}

#[test]
fn keyed_run_closure_is_per_value() {
    check(KEYED, CLO.0, CLO.1, "'a'", "'b'", &["A"]);
}

#[test]
fn unknown_string_runs_one_merged_context() {
    check(SW, OBJ.0, OBJ.1, "process.argv[2]", "process.argv[3]", &["A", "B"]);
    check(KEYED, CLO.0, CLO.1, "process.argv[2]", "process.argv[3]", &["A", "B"]);
}

#[test]
fn same_constant_shares_one_context() {
    check(SW, OBJ.0, OBJ.1, "'a'", "'a'", &["A", "B"]);
    check(KEYED, MAP.0, MAP.1, "'a'", "'a'", &["A", "B"]);
}
