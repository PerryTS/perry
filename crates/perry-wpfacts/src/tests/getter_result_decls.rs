//! Group-a getters the realm declares with their result type
//! (`effects = { returns(..) }` on the `.<get>` entry): a load on a builtin
//! object of an undescribed family (`family_load`) reads that type, not
//! "result of a getter the realm does not declare" (top).
//! Node: `/a/g.flags` and `.source` are strings; URL `protocol` a string.
//! A program that redefines the accessor on the prototype reads what its
//! own getter returns (the controls).
use super::*;
use crate::model::prim;

/// The kind of `H.v`, after `h.v = <read>` (`prelude` runs first).
fn stored(prelude: &str, read: &str) -> crate::derive::KindFact {
    let pf = facts(&format!(
        "class H {{ v: any = 0; }}\n\
         {prelude}\n\
         const h = new H();\n\
         h.v = {read};\n\
         console.log(h.v);\n"
    ));
    field(tmpl(&pf, "H"), "v").kind.clone()
}

/// The read is no top and adds exactly the primitives `want` (the initial
/// number of the field and the model's `undefined` aside).
fn declared(prelude: &str, read: &str, want: u16) {
    let k = stored(prelude, read);
    assert!(!k.top, "{read}: {k:?}");
    assert_eq!(
        k.prims & !(prim::NUM | prim::UNDEF),
        want & !(prim::NUM | prim::UNDEF),
        "{read}: {k:?}"
    );
}

#[test]
fn regexp_flags_and_source_are_strings() {
    declared("const r = /a/g;", "r.flags", prim::STR);
    declared("const r = /a/g;", "r.source", prim::STR);
    declared("const r = new RegExp(\"a\", \"g\");", "r.flags", prim::STR);
}

#[test]
fn url_protocol_is_a_string() {
    declared("const u = new URL(\"http://a/\");", "u.protocol", prim::STR);
}

/// After `Object.defineProperty(P, key, { get() { return true; } })` the
/// read is no longer only the declared result: the program's getter runs.
fn redefined(prelude: &str, proto: &str, key: &str, read: &str) {
    let k = stored(
        &format!(
            "{prelude}\nObject.defineProperty({proto}, \"{key}\", {{ get() {{ return true; }} }});"
        ),
        read,
    );
    assert!(k.top || k.prims & prim::BOOL != 0, "{read}: {k:?}");
}

#[test]
fn regexp_flags_redefined_on_the_prototype() {
    redefined("const r = /a/g;", "RegExp.prototype", "flags", "r.flags");
    redefined("const r = /a/g;", "RegExp.prototype", "source", "r.source");
}

#[test]
fn url_protocol_redefined_on_the_prototype() {
    redefined(
        "const u = new URL(\"http://a/\");",
        "URL.prototype",
        "protocol",
        "u.protocol",
    );
}
