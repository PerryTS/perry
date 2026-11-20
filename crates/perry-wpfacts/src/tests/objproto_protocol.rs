//! `Object.prototype.toString` reads `this[@@toStringTag]` (ECMA-262
//! 20.1.3.6 step 15: a getter there runs) and `Object.prototype.toLocaleString`
//! invokes `this.toString()` (20.1.3.5). A body only those built-ins run is
//! active; without the call it is not.

use super::*;

/// Whether `mark` can run in `src`.
fn mark_runs(src: &str) -> bool {
    let src = format!("function mark() {{ return 1; }}\n{src}");
    let module = perry_parser::parse_typescript(&src, "t.ts").expect("parses");
    let hir = perry_hir::lower_module(&module, "t", "t.ts").expect("lowers");
    let list = [("t.ts".to_string(), &hir)];
    let pf = crate::compute(&list, &FactsConfig::default());
    assert!(pf.body_reach.complete, "activated");
    let on = &pf.body_reach.bodies;
    let id = hir
        .functions
        .iter()
        .find(|f| f.name == "mark")
        .expect("mark")
        .id;
    on.iter().any(|(_, i, _)| *i == id)
}

const TAGGED: &str = r#"
const tagged = { get [Symbol.toStringTag]() { mark(); return "Tagged"; } };
"#;

#[test]
fn to_string_runs_the_to_string_tag_getter() {
    assert!(mark_runs(&format!(
        "{TAGGED}\nconsole.log(Object.prototype.toString.call(tagged));"
    )));
}

#[test]
fn an_unread_to_string_tag_getter_does_not_run() {
    assert!(!mark_runs(&format!(
        "{TAGGED}\nconsole.log(typeof tagged);"
    )));
}

const OWN_TO_STRING: &str = r#"
const o = { toString() { mark(); return "mine"; } };
"#;

#[test]
fn to_locale_string_invokes_to_string() {
    assert!(mark_runs(&format!(
        "{OWN_TO_STRING}\nconsole.log(o.toLocaleString());"
    )));
}

#[test]
fn an_uncalled_to_string_does_not_run() {
    assert!(!mark_runs(&format!(
        "{OWN_TO_STRING}\nconsole.log(typeof o);"
    )));
}
