//! Rule M1 (switch_param_runs).
//! A parameter that is never assigned (no `arguments`, no direct eval) and
//! is the discriminant of a switch whose case tests are all string
//! literals: a call passing exactly one string constant runs only the
//! clauses that constant selects (ECMA-262 14.12.4 CaseBlockEvaluation:
//! the first clause whose test is strictly equal, then fallthrough until a
//! break; the default clause when none matches).
use super::*;

fn src(get: &str, args: [&str; 3]) -> String {
    format!(
        "class A {{ p = 1; }}
         class B {{ p = 2; }}
         class C {{ p = 3; }}
         class D {{ p = 4; }}
         class P {{
           {get}
           go() {{
             const a: any = this.get({});
             const b: any = this.get({});
             const c: any = this.get({});
             console.log(a.p, b.p, c.p);
           }}
         }}
         new P().go();",
        args[0], args[1], args[2]
    )
}

const GET: &str = "get(t: string) { let r: any; switch (t) { case 'a': r = new A(); break; case 'b': r = new B(); break; case 'c': r = new C(); break; default: r = new D(); } return r; }";
const LITS: [&str; 3] = ["'a'", "'b'", "'c'"];

fn per_read(pf: &ProgramFacts) -> Vec<Vec<String>> {
    let mut v: Vec<Vec<String>> = pf
        .sites
        .iter()
        .filter(|s| s.key == "p" && !s.is_call)
        .map(|s| {
            let mut r: Vec<String> = s.receivers.iter().map(|r| r.rsplit(':').next().unwrap_or(r).to_string()).collect();
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
fn switch_parameter_runs_per_string_constant() {
    assert_eq!(per_read(&facts(&src(GET, LITS))), vec![vec!["A"], vec!["B"], vec!["C"]]);
}

#[test]
fn unknown_string_keeps_every_case() {
    let v = per_read(&facts(&src(GET, ["process.argv[2]", "'b'", "'c'"])));
    assert!(v.iter().any(|r| r.len() == 4), "{v:?}");
}

#[test]
fn two_constants_keep_both_cases() {
    let v = per_read(&facts(&src(GET, ["process.argv[2] ? 'a' : 'b'", "'b'", "'c'"])));
    assert!(v.iter().any(|r| r.contains(&"A".to_string()) && r.contains(&"B".to_string())), "{v:?}");
}

#[test]
fn assigned_parameter_keeps_every_case() {
    let g = GET.replace("let r: any;", "let r: any; if (process.argv[3]) t = 'c';");
    assert!(!split(&per_read(&facts(&src(&g, LITS)))));
}

#[test]
fn fallthrough_holds_both_blocks() {
    let g = GET.replace("r = new A(); break;", "r = new A();");
    let v = per_read(&facts(&src(&g, LITS)));
    // 'a' falls into 'b': r is A or B there; 'b' and 'c' stay alone.
    assert_eq!(v, vec![vec!["A", "B"], vec!["B"], vec!["C"]]);
}

#[test]
fn no_match_runs_default_only() {
    let v = per_read(&facts(&src(GET, ["'z'", "'b'", "'c'"])));
    assert_eq!(v, vec![vec!["B"], vec!["C"], vec!["D"]]);
}

#[test]
fn switch_under_if_runs_per_string_constant() {
    let g = GET.replace("let r: any; switch (t) {", "let r: any; if (!r) { switch (t) {").replace("r = new D(); }", "r = new D(); } }");
    assert_eq!(per_read(&facts(&src(&g, LITS))), vec![vec!["A"], vec!["B"], vec!["C"]]);
}

#[test]
fn switch_in_loop_keeps_every_case() {
    let g = GET.replace("let r: any; switch (t) {", "let r: any; for (let i = 0; i < 1; i++) { switch (t) {").replace("r = new D(); }", "r = new D(); } }");
    assert!(!split(&per_read(&facts(&src(&g, LITS)))));
}
