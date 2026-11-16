//! A match's `groups` object is created with a null prototype (ECMA-262
//! 22.2.7.2 RegExpBuiltinExec step 34: OrdinaryObjectCreate(null); the
//! runtime: `regex/perex_results.rs` `null_groups`). A computed key it does
//! not own reads `undefined` (10.1.8.1 OrdinaryGet step 3.b): its own
//! values (Strings or undefined, or what the program stored), never a
//! builtin member. Calling what such a read gives hands its receiver and
//! arguments to no builtin function. Control: a value the program stored
//! under the key is still read.
use super::*;

const PRE: &str = r#"
    class Svc { marker = 3; }
    function g(a: any) { return a.marker; }
    const k: any = String(Math.random());
    const s: any = String(Math.random());
    const m: any = /(?<a>x)(?<b>y)?/.exec(s);
"#;

fn proven(body: &str) -> bool {
    let src = format!("{PRE}\nconst o: any = new Svc();\n{body}\nconsole.log(g(o));\n");
    let pf = facts(&src);
    let v = &site(&pf, "marker").verdict;
    !matches!(v, Verdict::No(_))
}

fn uses() -> Vec<&'static str> {
    vec![
        "try { const v: any = m.groups[k]; v(o); } catch (e) {}",
        "try { const v: any = m.groups[k]; v.call(o, o); } catch (e) {}",
        "try { Reflect.apply(m.groups[k], o, [o]); } catch (e) {}",
        "try { const v: any = m.groups[k]; console.log(\"\" + v); v(o); } catch (e) {}",
    ]
}

/// A computed key of a match's groups object names no builtin function.
#[test]
fn a_null_prototype_groups_key_reads_no_builtin_member() {
    let bad: Vec<&str> = uses().into_iter().filter(|b| !proven(b)).collect();
    assert!(bad.is_empty(), "escaped through a groups computed key: {bad:#?}");
}

/// Control (the fixture can fail): a value the program stored on the
/// groups object under the key is read and called.
#[test]
fn a_value_stored_on_the_groups_object_is_read() {
    for b in uses() {
        let b = format!("if (m) m.groups[k] = (globalThis as any).hostGet();\n{b}");
        assert!(!proven(&b), "{b}");
    }
}
