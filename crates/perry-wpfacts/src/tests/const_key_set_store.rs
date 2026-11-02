//! A store under a computed key that is one of a few constant strings
//! (`const k = w ? "a" : "b"; o[k] = v`) writes one of those keys and no
//! other: it is a named store under each. Through ⊤ (which may be a builtin
//! prototype) it writes those keys of the runtime objects, not every key.

use super::*;

/// `h` is a value from unmodeled code (node: `hostFn` returns
/// `Array.prototype`). `a[0]` is an element read on an array, which
/// consults the builtin prototypes under its index.
fn verdict(store: &str, read: &str) -> Verdict {
    let src = format!(
        r#"
        declare const w: boolean;
        declare const name: string;
        const h: any = (globalThis as any).hostFn();
        {store}
        class Svc {{ size = 3; }}
        const a: any = [new Svc()];
        console.log(a[0].size);
        console.log(a.forWriting === undefined ? 0 : 1);
        "#
    );
    site(&facts(&src), read).verdict.clone()
}

fn proven(v: &Verdict) -> bool {
    !matches!(v, Verdict::No(_))
}

const SET: &str = r#"const k = w ? "forWriting" : "forReading"; h[k] = 1;"#;

#[test]
fn a_constant_key_set_store_writes_only_its_keys() {
    let v = verdict(SET, "size");
    assert!(proven(&v), "{v:?}");
}

#[test]
fn a_key_of_the_constant_set_is_written() {
    // node: `[].forWriting` is 1 once the store went to `Array.prototype`.
    let v = verdict(SET, "forWriting");
    assert!(!proven(&v), "{v:?}");
}

#[test]
fn an_unknown_key_store_writes_every_key() {
    let v = verdict("h[w ? name : \"forReading\"] = 1;", "size");
    assert!(!proven(&v), "{v:?}");
}
