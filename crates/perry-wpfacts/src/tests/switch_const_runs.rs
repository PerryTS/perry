//! Rule M1 when the switched argument fills late: the call waits for its
//! argument (`settle_val_runs`), which then holds two or more string
//! constants; each constant runs only the clauses it selects (ECMA-262
//! 14.12.4 CaseBlockEvaluation), also when a clause constructs a typed
//! array from a parameter-derived length (the three.js PropertyMixer
//! constructor shape: `switch (typeName)` with `new Float64Array(n * 6)`).
use super::*;

/// `body` is M's constructor body over `t` and `n`; `kinds` are the `T`
/// field values of the classes whose instances reach `new M(x.T, 1)` through a
/// field load (the argument holds nothing when the call is first bound);
/// `extra` runs after.
fn src(body: &str, kinds: &[&str], extra: &str) -> String {
    let mut cls = String::new();
    let mut inst = Vec::new();
    for (i, k) in kinds.iter().enumerate() {
        cls.push_str(&format!("class K{i} {{ T = {k}; }}\n"));
        inst.push(format!("new K{i}()"));
    }
    format!(
        "class A {{ p = 1; }}
         class B {{ p = 2; }}
         class D {{ p = 4; }}
         {cls}
         class M {{ b: any; c: any; e: any; constructor(t: string, n: number) {{ {body} }} }}
         function mk(x: any): any {{ return new M(x.T, 1); }}
         const xs: any[] = [{}];
         for (const x of xs) {{ const m = mk(x); console.log(m.c.p, m.e.p, m.b.length); }}
         {extra}",
        inst.join(", ")
    )
}

const LIT: &str = "switch (t) { case 'q': this.b = { k: 1 }; this.c = new A(); break; case 's': case 'b': this.b = [n]; this.c = new B(); break; default: this.b = { k: 2 }; this.c = new D(); }";
const TA: &str = "switch (t) { case 'quaternion': this.b = new Float64Array(n * 6); this.c = new A(); break; case 'string': case 'bool': this.b = new Array(n * 5); this.c = new B(); break; default: this.b = new Float64Array(n * 5); this.c = new D(); }";
const FALL: &str = "switch (t) { case 'q': this.c = new A(); break; case 's': this.e = new D(); case 'b': this.c = new B(); break; default: this.c = new D(); }";

/// The classes the reads of `key` see (`m.c.p`: key "p" on field c).
fn recv(pf: &ProgramFacts, key: &str) -> Vec<String> {
    let mut v: Vec<String> = pf
        .sites
        .iter()
        .filter(|s| s.key == key && !s.is_call)
        .flat_map(|s| s.receivers.iter().map(|r| r.rsplit(':').next().unwrap_or(r).to_string()))
        .collect();
    v.sort();
    v.dedup();
    v
}

fn has_array(v: &[String]) -> bool {
    v.iter().any(|r| r.starts_with("Array"))
}

// P2 alone: two constants that arrive late; the 's'/'b' clauses never run.
#[test]
fn late_constants_run_their_clauses_only() {
    let pf = facts(&src(LIT, &["'number'", "'q'"], ""));
    let c = recv(&pf, "p");
    let l = recv(&pf, "length");
    assert!(!c.contains(&"B".to_string()) && c.contains(&"A".to_string()) && c.contains(&"D".to_string()), "{c:?}");
    assert!(!has_array(&l), "{l:?}");
}

// P1 + P2: the clauses construct typed arrays from `n`.
#[test]
#[ignore = "P1: the typed-array construct refuses the summary"]
fn late_constants_with_typed_array_construct() {
    let pf = facts(&src(TA, &["'number'", "'quaternion'", "'vector'"], ""));
    let c = recv(&pf, "p");
    let l = recv(&pf, "length");
    assert!(!c.contains(&"B".to_string()) && c.contains(&"A".to_string()) && c.contains(&"D".to_string()), "{c:?}");
    assert!(!has_array(&l), "{l:?}");
}

// Control 1: 'string' is one of the constants: its clause runs.
#[test]
#[ignore = "P1: the typed-array construct refuses the summary"]
fn constant_of_the_array_clause_runs_it() {
    let pf = facts(&src(TA, &["'number'", "'string'"], ""));
    let c = recv(&pf, "p");
    let l = recv(&pf, "length");
    assert!(c.contains(&"B".to_string()) && c.contains(&"D".to_string()) && !c.contains(&"A".to_string()), "{c:?}");
    assert!(has_array(&l), "{l:?}");
}

// Control 2: an unknown string: every clause is live.
#[test]
fn unknown_string_runs_every_clause() {
    let pf = facts(&src(TA, &["process.argv[2]", "'quaternion'"], ""));
    let c = recv(&pf, "p");
    assert!(["A", "B", "D"].iter().all(|k| c.contains(&k.to_string())), "{c:?}");
    assert!(has_array(&recv(&pf, "length")));
}

// Control 3: fallthrough: 's' runs its own block and 'b''s; 'b' only its own.
#[test]
fn fallthrough_from_s_runs_b() {
    let pf = facts(&src(FALL, &["'s'", "'q'"], ""));
    let c = recv(&pf, "p");
    // c is A ('q') or B ('s' falls into 'b'); e is D only from 's'.
    assert!(c.contains(&"A".to_string()) && c.contains(&"B".to_string()) && c.contains(&"D".to_string()), "{c:?}");
    let pf = facts(&src(FALL, &["'b'", "'q'"], ""));
    let c = recv(&pf, "p");
    // No 's': e is never stored, c never D (default unreached).
    assert!(c.contains(&"A".to_string()) && c.contains(&"B".to_string()) && !c.contains(&"D".to_string()), "{c:?}");
}

// Control 4: a second call path passes an unknown string later: the
// shared run is taken and the Array clause is there.
#[test]
fn later_unknown_string_rebinds() {
    let pf = facts(&src(TA, &["'number'", "'quaternion'"], "class Z { T = process.argv[2]; }\nconst z: any = mk(new Z()); console.log(z.c.p);"));
    let c = recv(&pf, "p");
    assert!(c.contains(&"B".to_string()), "{c:?}");
    assert!(has_array(&recv(&pf, "length")));
}
