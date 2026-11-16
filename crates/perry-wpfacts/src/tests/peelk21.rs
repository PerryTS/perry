//! PEELK21 (Response body error): `new Response(body)` (Fetch 6.4 Response
//! constructor, extract a body): a body read (`text()`, `arrayBuffer()`, ...)
//! rejects with a TypeError (body used or locked), a DOMException, or, for a
//! ReadableStream body, the stream's stored error (Streams 4.9.x
//! ReadableStreamError: what `controller.error(e)` passed; a rejected start/pull
//! promise rejects with a program value already thrown). Any other body has no
//! program-made error. Node: undici lib/web/fetch/body.js, response.js.
use super::*;

fn whole(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

fn escape_with_field(pf: &ProgramFacts, key: &str) -> Option<String> {
    pf.templates
        .iter()
        .filter(|t| t.display.starts_with("t.ts"))
        .find(|t| t.fields.iter().any(|f| f.key == key))
        .unwrap_or_else(|| panic!("no t.ts template with field {key}\n{}", pf.dump_text()))
        .escape
        .clone()
}

fn src(err: &str, body: &str) -> String {
    format!(
        "const lit: any = {{ keptmark: 1 }};
         const err: any = {err};
         const body: any = {body};
         new Response(body).text().catch(function (e: any) {{ e.f(lit); }});"
    )
}

const ERR: &str = "{ f(x: any) { return x; } }";
const STREAM: &str = "new ReadableStream({ start(c: any) { c.error(err); } })";

#[test]
fn stream_body_error_is_the_stored_error() {
    let pf = whole(&src(ERR, STREAM));
    assert_eq!(escape_with_field(&pf, "keptmark"), None, "{}", pf.dump_text());
}

#[test]
fn string_body_has_no_program_error() {
    let pf = whole(&src(ERR, "\"abc\""));
    assert_eq!(escape_with_field(&pf, "keptmark"), None, "{}", pf.dump_text());
}

#[test]
fn clone_keeps_the_stored_error() {
    let pf = whole(&format!(
        "const lit: any = {{ keptmark: 1 }};
         const err: any = {ERR};
         new Response({STREAM}).clone().arrayBuffer().catch(function (e: any) {{ e.f(lit); }});"
    ));
    assert_eq!(escape_with_field(&pf, "keptmark"), None, "{}", pf.dump_text());
}

#[test]
fn control_unknown_stored_error_escapes() {
    let pf = whole(&src("new Function(process.argv[2])()", STREAM));
    assert!(escape_with_field(&pf, "keptmark").is_some(), "{}", pf.dump_text());
}
