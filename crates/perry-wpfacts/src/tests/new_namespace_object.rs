//! `new` on a namespace object (the global object, `Math`, `JSON`,
//! `Reflect`, `console`, `process`) constructs nothing: each is an ordinary
//! object without [[Construct]], so `new X` throws a TypeError at
//! IsConstructor (ECMA-262 13.3.5.1.1 step 7; 19, 21.3, 25.5, 28.1). It
//! mints no object, so no "builtin" token reaches another `new`.
//! Control: `new` on a class still constructs.
use super::*;

fn get_proven(tail: &str) -> bool {
    let src = format!(
        r#"
        class Svc {{ size = 3; }}
        const s = new Svc();
        const o: any = globalThis;
        const k: any = process.argv[2];
        o[k] = globalThis;
        {tail}
        const m = new Map<any, any>();
        m.set(s, 1);
        console.log(m.get(s));
        "#
    );
    let pf = facts(&src);
    !matches!(site(&pf, "get").verdict, Verdict::No(_))
}

#[test]
fn new_globalthis_constructs_nothing() {
    assert!(get_proven(
        "try { new (globalThis as any)(); } catch (e) {}"
    ));
}

#[test]
fn new_math_constructs_nothing() {
    assert!(get_proven("try { new (Math as any)(); } catch (e) {}"));
}

#[test]
fn new_map_keeps_its_receiver_exact() {
    assert!(get_proven(""));
}

#[test]
fn new_class_still_constructs() {
    let pf = facts(
        r#"
        class Svc { size = 3; }
        const q: any = new (class { z = 1; })();
        console.log(q.z);
        "#,
    );
    assert!(!matches!(site(&pf, "z").verdict, Verdict::No(_)));
}
