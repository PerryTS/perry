//! Rule M1 (switch_param_runs): an item the body makes both inside a
//! clause and outside every clause runs in every run. A run whose constant
//! does not reach the clause still executes the statement outside it
//! (ECMA-262 14.12.4: the clauses a constant selects; statements before and
//! after the switch run on every call).
use super::*;

fn src(get: &str) -> String {
    format!(
        "class A {{ p = 1; q = 1; }}
         class B {{ p = 2; q = 2; }}
         function sink(o: any) {{ console.log(o.q); }}
         class P {{
           h: any = new A();
           {get}
         }}
         const x = new P();
         const ra: any = x.get('a', new A());
         const rb: any = x.get('b', new B());
         console.log(ra.p, rb.p, x.h.p);",
    )
}

fn recv(pf: &ProgramFacts, key: &str) -> Vec<Vec<String>> {
    let mut v: Vec<Vec<String>> = pf
        .sites
        .iter()
        .filter(|s| s.key == key && !s.is_call)
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

fn has_b(v: &[Vec<String>]) -> bool {
    v.iter().any(|r| r.contains(&"B".to_string()))
}

const SW: &str = "switch (t) { case 'a': STMT; break; case 'b': break; }";

fn get(before: &str, stmt: &str, after: &str) -> String {
    format!("get(t: string, v: any) {{ let r: any = null; {before} {} {after} return r; }}", SW.replace("STMT", stmt))
}

#[test]
fn store_before_and_in_clause_reaches_every_run() {
    let pf = facts(&src(&get("this.h = v;", "this.h = v", "")));
    assert!(has_b(&recv(&pf, "p")), "{:?}", recv(&pf, "p"));
}

#[test]
fn store_in_clause_and_after_reaches_every_run() {
    let pf = facts(&src(&get("", "this.h = v", "this.h = v;")));
    assert!(has_b(&recv(&pf, "p")), "{:?}", recv(&pf, "p"));
}

#[test]
fn copy_before_and_in_clause_reaches_every_run() {
    let pf = facts(&src(&get("r = v;", "r = v", "")));
    assert!(has_b(&recv(&pf, "p")), "{:?}", recv(&pf, "p"));
}

#[test]
fn copy_in_clause_and_after_reaches_every_run() {
    let pf = facts(&src(&get("", "r = v", "r = v;")));
    assert!(has_b(&recv(&pf, "p")), "{:?}", recv(&pf, "p"));
}

#[test]
fn load_in_clause_and_after_reaches_every_run() {
    // rb is this.h read after the switch: the b run must read it too.
    let pf = facts(&src(&get("this.h = v;", "r = this.h", "r = this.h;")));
    assert!(has_b(&recv(&pf, "p")), "{:?}", recv(&pf, "p"));
}

#[test]
fn call_before_and_in_clause_reaches_every_run() {
    let pf = facts(&src(&get("sink(v);", "sink(v)", "")));
    assert!(has_b(&recv(&pf, "q")), "{:?}", recv(&pf, "q"));
}

#[test]
fn call_in_clause_and_after_reaches_every_run() {
    let pf = facts(&src(&get("", "sink(v)", "sink(v);")));
    assert!(has_b(&recv(&pf, "q")), "{:?}", recv(&pf, "q"));
}

#[test]
fn clause_only_store_stays_out_of_other_runs() {
    // Control: the store only in case 'a'; the 'b' run returns null.
    let pf = facts(&src(&get("", "r = v", "")));
    let v = recv(&pf, "p");
    assert!(v.iter().all(|r| !r.contains(&"B".to_string()) || r.len() == 1), "{v:?}");
}
