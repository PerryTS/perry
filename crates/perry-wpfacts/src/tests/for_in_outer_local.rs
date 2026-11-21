//! An outlined for-in body (`Gen::for_in_body`) runs the enclosing
//! function's statements, so its write of that function's local reaches
//! the reads after the loop. `let r = 0;` starts a narrowing of `r`
//! (`Gen::narrow_assigned`): the reads after it see its end node, and a
//! write in a nested statement must reach that end (`narrowed_write`).
//! The body is its own function, so before the fix its write missed the
//! end and `return r` held only `0` (a compiled `x.zz()` threw
//! "(number).zz is not a function").
use super::*;

const MK: &str = "class Mk { zz() { return 1; } }\nclass Nk { zz() { return 2; } }\n";

/// The program receivers of `x.zz()`.
fn zz(body: &str) -> Vec<String> {
    let pf = facts(&format!("{MK}{body}"));
    let s = site(&pf, "zz");
    let mut r: Vec<String> = s
        .receivers
        .iter()
        .filter(|r| r.starts_with("t.ts:"))
        .cloned()
        .collect();
    r.sort();
    eprintln!("FORIN-OUTER {body:?} top={} recv={r:?}", s.top);
    r
}

#[test]
fn a_named_read_in_the_body_reaches_the_return() {
    let r = zz("function w(src: any) { let r: any = 0; for (const p in src) { r = src.a; } return r; }\nconst x = w({ a: new Mk() }); console.log(x.zz());\n");
    assert!(r.contains(&"t.ts:Mk".to_string()), "{r:?}");
}

#[test]
fn a_keyed_read_in_the_body_reaches_the_return() {
    let r = zz("function walk(src: any) { let r: any = 0; for (const p in src) { r = src[p]; } return r; }\nconst x = walk({ a: new Mk() }); console.log(x.zz());\n");
    assert!(r.contains(&"t.ts:Mk".to_string()), "{r:?}");
}

#[test]
fn a_class_instance_walked_reaches_the_return() {
    let r = zz("class Hd { a: any = new Mk(); }\nfunction walk(src: any) { let r: any = 0; for (const p in src) { r = src[p]; } return r; }\nconst x = walk(new Hd()); console.log(x.zz());\n");
    assert!(r.contains(&"t.ts:Mk".to_string()), "{r:?}");
}

/// Two calls: each run of the enclosing function gets its own body runs,
/// and both values reach the site.
#[test]
fn each_call_of_the_enclosing_function_gets_its_value() {
    let r = zz("function walk(src: any) { let r: any = 0; for (const p in src) { r = src[p]; } return r; }\nconst x = walk({ a: new Mk() }); const y = walk({ b: new Nk() }); console.log(x.zz(), y.zz());\n");
    assert!(r.contains(&"t.ts:Mk".to_string()) && r.contains(&"t.ts:Nk".to_string()), "{r:?}");
}

/// The write inside an `if` of the body reaches the end too.
#[test]
fn a_write_under_a_test_in_the_body_reaches_the_return() {
    let r = zz("function w(src: any) { let r: any = 0; for (const p in src) { if (p === \"a\") { r = src[p]; } } return r; }\nconst x = w({ a: new Mk() }); console.log(x.zz());\n");
    assert!(r.contains(&"t.ts:Mk".to_string()), "{r:?}");
}

/// `let r;` with no initializer: the same.
#[test]
fn a_local_without_initializer_reaches_the_return() {
    let r = zz("function walk(src: any) { let r: any; for (const p in src) { r = src[p]; } return r; }\nconst x = walk({ a: new Mk() }); console.log(x.zz());\n");
    assert!(r.contains(&"t.ts:Mk".to_string()), "{r:?}");
}
