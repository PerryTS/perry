//! A spread argument yields the iterated elements (CreateListFromIterable),
//! never a method a computed key could name: spreading an array passes its
//! elements, not an opaque builtin function. A spread of a value from
//! unmodeled code passes ⊤.

use super::*;

fn verdict(spread: &str) -> Verdict {
    let src = format!(
        r#"
        class Svc {{ size = 3; }}
        function g(a: any) {{ return a.size; }}
        {spread}
        "#
    );
    site(&facts(&src), "size").verdict.clone()
}

fn proven(v: &Verdict) -> bool {
    !matches!(v, Verdict::No(_))
}

/// node: prints `3`.
#[test]
fn a_spread_array_passes_its_elements() {
    let v = verdict("const xs = [new Svc()]; console.log(g(...xs));");
    assert!(proven(&v), "{v:?}");
}

/// node (`hostList = [{ size: 9 }]`): prints `9`.
#[test]
fn a_spread_top_value_passes_top() {
    let v = verdict("const xs: any = (globalThis as any).hostList; console.log(g(...xs));");
    assert!(!proven(&v), "{v:?}");
}

/// MM9: the reads of `key`: receivers, whether one holds ⊤, whether every
/// site is proven.
fn mm9_reads(src: &str, key: &str) -> (Vec<String>, bool, bool) {
    let pf = facts(src);
    let ss: Vec<_> = pf.sites.iter().filter(|s| s.key == key).collect();
    assert!(!ss.is_empty(), "site {key} in\n{}", pf.dump_text());
    let mut got: Vec<String> = ss.iter().flat_map(|s| s.receivers.clone()).collect();
    got.sort();
    got.dedup();
    let top = ss.iter().any(|s| s.top);
    let proven = ss.iter().all(|s| !matches!(s.verdict, Verdict::No(_)));
    (got, top, proven)
}

fn mm9_src(body: &str) -> String {
    format!(
        r#"
class Svc {{ run() {{ return 1; }} }}
const j: any = JSON.parse((globalThis as any).host);
{body}
"#
    )
}

/// A primitive's wrapper (`Object(x)`, `new Object(x)`; ECMA-262 20.1.1.1,
/// 7.1.18 ToObject) is born with no own enumerable property but a String
/// wrapper's indices (10.4.3): a copy of it holds what the program stored
/// on it, never the unknown value. (Three.js GLTFLoader: `new TypedArray(..)`
/// whose callee set holds `Object`, its result copied into a literal.)
/// node (`host = '{"n":1,"s":"ab"}'`): prints `1` twice.
#[test]
fn red_mm9_spread_of_a_primitive_wrapper_copies_only_its_stores() {
    for body in [
        "const o: any = { svc: new Svc(), ...Object(+j.n) }; console.log(o.svc.run());",
        "const o: any = Object.assign({ svc: new Svc() }, new Object(+j.n)); console.log(o.svc.run());",
        "const o: any = Object.assign({ svc: new Svc() }, Object(String(j.s))); console.log(o.svc.run());",
    ] {
        let (got, top, proven) = mm9_reads(&mm9_src(body), "run");
        assert!(!top && proven, "{body}: {got:?} top={top} proven={proven}");
    }
}

/// Control (i): a store on the wrapper is copied. node: prints `1`.
#[test]
fn mm9_a_wrapper_store_is_copied() {
    let body = "const w: any = Object(5); w.a = new Svc(); const o: any = { ...w }; console.log(o.a.run());";
    let (got, top, proven) = mm9_reads(&mm9_src(body), "run");
    assert!(!top && proven && got.iter().any(|g| g.contains("Svc")), "{got:?} top={top}");
}

/// Control (ii): a wrapper given to unknown code first may hold anything.
/// node (`sink = w => { w.svc = 7 }`): throws (7 has no `run`).
#[test]
fn mm9_an_escaped_wrapper_copies_unknown() {
    let body = "const w: any = Object(5); (globalThis as any).sink(w); const o: any = { svc: new Svc(), ...w }; console.log(o.svc.run());";
    let (got, top, _) = mm9_reads(&mm9_src(body), "run");
    assert!(top, "{got:?}");
}

/// Control (iii): a String wrapper's indices are copied (index keys, each a
/// one-character string), never a name. node: prints `1 a`.
#[test]
fn mm9_a_string_wrapper_copies_its_indices_only() {
    let body = "const o: any = { svc: new Svc(), ...new String(String(j.s)) }; console.log(o.svc.run(), o[0].codePointAt(0));";
    let (got, top, proven) = mm9_reads(&mm9_src(body), "run");
    assert!(!top && proven, "{got:?} top={top}");
    let (got, top, _) = mm9_reads(&mm9_src(body), "codePointAt");
    assert!(!top, "{got:?}");
}
