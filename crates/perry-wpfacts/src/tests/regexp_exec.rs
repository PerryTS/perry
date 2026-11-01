//! `RegExp.prototype.test`, `@@replace`, `@@match` and `@@search` run the
//! regexp's `exec` through a property lookup (RegExpExec, 22.2.7.1): an
//! `exec` stored on the object runs, with the regexp as `this`. `@@split` and
//! `@@matchAll` construct through the species constructor: a `constructor`
//! stored on the object runs.

use super::*;

fn verdict(body: &str) -> Verdict {
    let src = format!(
        r#"
        class Svc {{ size = 3; }}
        function g(a: any) {{ return a.size; }}
        const re = /b/;
        {body}
        "#
    );
    let pf = facts(&src);
    let v: Vec<_> = pf.sites.iter().filter(|s| s.key == "size").collect();
    assert_eq!(v.len(), 1, "{}", pf.dump_text());
    v[0].verdict.clone()
}

fn proven(v: &Verdict) -> bool {
    !matches!(v, Verdict::No(_))
}

/// `use_it` runs on a regexp whose `exec` stores the `Svc` it closes over
/// into unknown code: node runs that `exec`, so the object escapes.
fn runs_replaced_exec(use_it: &str) {
    let v = verdict(&format!(
        "const o = new Svc();
         (re as any).exec = function (_x: string) {{ (globalThis as any).hostTake(o); return null; }};
         {use_it}; console.log(g(o));"
    ));
    assert!(!proven(&v), "{use_it}: {v:?}");
}

#[test]
fn test_runs_a_replaced_exec() {
    runs_replaced_exec("re.test(b)");
}

#[test]
fn replace_runs_a_replaced_exec() {
    runs_replaced_exec("abc.replace(re, x)");
}

#[test]
fn match_runs_a_replaced_exec() {
    runs_replaced_exec("abc.match(re)");
}

#[test]
fn search_runs_a_replaced_exec() {
    runs_replaced_exec("abc.search(re)");
}

#[test]
fn symbol_replace_runs_a_replaced_exec() {
    runs_replaced_exec("re[Symbol.replace](abc, x)");
}

/// The control: with the builtin `exec` nothing user-written runs.
#[test]
fn test_with_the_builtin_exec_is_proven() {
    let v = verdict("const o = new Svc(); re.test(b); console.log(g(o));");
    assert!(proven(&v), "{v:?}");
}

fn runs_stored_species_constructor(use_it: &str) {
    let v = verdict(&format!(
        "const o = new Svc(); const rg = /b/g;
         const C: any = function () {{ (globalThis as any).hostTake(o); }};
         C[Symbol.species] = C; (rg as any).constructor = C;
         try {{ {use_it}; }} catch (_e) {{}} console.log(g(o));"
    ));
    assert!(!proven(&v), "{use_it}: {v:?}");
}

#[test]
fn split_runs_a_stored_species_constructor() {
    runs_stored_species_constructor("'a-b'.split(rg)");
}

#[test]
fn match_all_runs_a_stored_species_constructor() {
    runs_stored_species_constructor("'a-b'.matchAll(rg)");
}

#[test]
fn split_with_the_default_constructor_is_proven() {
    let v = verdict("const o = new Svc(); 'a-b'.split(re); console.log(g(o));");
    assert!(proven(&v), "{v:?}");
}
