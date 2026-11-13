//! `Error.prototype` and each NativeError prototype own `name` and
//! `message` as writable String data properties (ECMA-262 20.5.3.2,
//! 20.5.3.3, 20.5.6.3.2, 20.5.6.3.3; the runtime: `proto_methods.rs`
//! `install_error_prototype_data_properties`, a String each). Reading them
//! on such a prototype, or through it on an error object without an own
//! value (the TypeError a rejected `fetch` makes), gives a String: calling
//! or converting what such a read gives hands its receiver and arguments to
//! no builtin function. Control: they are writable, so a value the program
//! stored on the prototype under the key is read.
use super::*;

const PRE: &str = r#"
    class Svc { marker = 3; }
    function g(a: any) { return a.marker; }
    const k: any = String(Math.random());
"#;

fn proven(body: &str) -> bool {
    let src = format!("{PRE}\nconst o: any = new Svc();\n{body}\nconsole.log(g(o));\n");
    let pf = facts(&src);
    let v = &site(&pf, "marker").verdict;
    !matches!(v, Verdict::No(_))
}

fn uses() -> Vec<&'static str> {
    vec![
        "fetch(k).catch((e: any) => { try { const v: any = e.name; v(o); } catch (x) {} });",
        "fetch(k).catch((e: any) => { try { const v: any = e.message; v.call(o, o); } catch (x) {} });",
        "fetch(k).catch((e: any) => { try { const v: any = e.name; console.log(\"\" + v); Reflect.apply(v, o, [o]); } catch (x) {} });",
    ]
}

/// `name`/`message` of an Error prototype read a String, never a builtin
/// function.
#[test]
fn an_error_prototype_name_or_message_reads_a_string() {
    let bad: Vec<&str> = uses().into_iter().filter(|b| !proven(b)).collect();
    assert!(bad.is_empty(), "escaped through an Error prototype data key: {bad:#?}");
}

/// Control (the fixture can fail): the properties are writable; a value
/// the program stored on `TypeError.prototype` is read and called.
#[test]
fn a_value_stored_on_the_error_prototype_is_read() {
    for b in uses() {
        let b = format!(
            "(TypeError.prototype as any).name = (globalThis as any).hostGet();\n(TypeError.prototype as any).message = (globalThis as any).hostGet();\n{b}"
        );
        assert!(!proven(&b), "{b}");
    }
}
