//! ECMA-262 13.3.2.1, 7.3.2, 10.1.8.1: `O[P]` on an ordinary object is
//! ToPropertyKey, then OrdinaryGet: property P and its chain only. A
//! program prototype (`V.prototype`) is an ordinary object, so a computed
//! read on it by a key whose values are known reads only the names the key
//! admits: not its methods, not its `constructor`. `r` goes to the host;
//! if it may hold `V::copy` (or class `V`, whose constructor the host may
//! run) the host may run code that hands `svc` out, and the facts must not
//! prove `svc.size`.
use super::*;

fn escapes(copy: &str, ctor: &str, key: &str) -> bool {
    let src = format!(
        r#"
        class Svc {{ size = 3; }}
        const svc: any = new Svc();
        class V {{ constructor() {{ {ctor} }} copy() {{ {copy} return 1; }} }}
        class B {{
            o: any; k: any;
            constructor(o: any, k: any) {{ this.o = o; this.k = k; }}
            get() {{ return this.o[this.k]; }}
        }}
        const b = new B(Math.random() < 0.5 ? V.prototype : {{ a: 1 }}, {key});
        const r = b.get();
        (globalThis as any).hostTake(r);
        function g(a: any) {{ return a.size; }}
        console.log(g(svc));
        "#
    );
    let v = site(&facts(&src), "size").verdict.clone();
    matches!(v, Verdict::No(_))
}

const LEAK: &str = "(globalThis as any).hostTake(svc);";
const AB: &str = r#"Math.random() < 0.5 ? "a" : "b""#;
const UNKNOWN: &str = "process.argv[2]";

/// The key is "a" or "b": `r` holds no `V::copy`.
#[test]
fn known_keys_read_no_prototype_method() {
    assert!(!escapes(LEAK, "", AB));
}

/// The key is "a" or "b": `r` holds no class `V` (the `constructor`).
#[test]
fn known_keys_read_no_prototype_constructor() {
    assert!(!escapes("", LEAK, AB));
}

/// Control: the key "copy" reads `V::copy`.
#[test]
fn control_key_copy_reads_the_method() {
    assert!(escapes(LEAK, "", r#""copy""#));
}

/// Control: a key of unknown content reads `V::copy` ...
#[test]
fn control_unknown_key_reads_the_method() {
    assert!(escapes(LEAK, "", UNKNOWN));
}

/// ... and class `V` (the `constructor`).
#[test]
fn control_unknown_key_reads_the_constructor() {
    assert!(escapes("", LEAK, UNKNOWN));
}
