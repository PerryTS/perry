//! `Error.prototype.toString` (ECMA-262 20.5.3.4) on an Error subclass
//! instance (three.js FileLoader: `class HttpError extends Error {
//! constructor(message, response) { super(message); this.response = response } }`,
//! thrown from a `fetch(..).then(..)` callback, then ToString'd).
//! Node: ToPrimitive on the instance finds `Error.prototype.toString`, which
//! is generic over an object `this`: it reads `name` and `message` through
//! [[Get]] and ToString's each; nothing else is called, nothing is kept.
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

fn src(tail: &str) -> String {
    format!(
        "const unknown: any = (globalThis as any)[process.argv[2]];
         class HttpError extends Error {{
           response: any;
           constructor(message: string, response: any) {{ super(message); this.response = response; }}
         }}
         const resp = {{ respmark: 1 }};
         fetch(process.argv[3]).then(() => {{
           throw new HttpError(`fetch failed ${{process.argv[4]}}`, resp);
         }}).catch((e: any) => {{
           {tail}
         }});"
    )
}

/// FileLoader's HttpError, ToString'd: neither it nor its response escapes.
#[test]
fn an_error_subclass_instance_tostring_hands_nothing_out() {
    let pf = whole(&src("console.log(`${e}`); console.log(String(e), e.response.respmark);"));
    assert_eq!(escape_with_field(&pf, "response"), None, "{}", pf.dump_text());
    assert_eq!(escape_with_field(&pf, "respmark"), None, "{}", pf.dump_text());
}

/// Called directly on an ordinary object: generic over `this`.
#[test]
fn error_tostring_on_a_plain_object_hands_nothing_out() {
    let pf = whole(&src(
        "const o = { plainmark: 1, name: 'N', message: 'M' };
         console.log(Error.prototype.toString.call(o), e);",
    ));
    assert_eq!(escape_with_field(&pf, "plainmark"), None, "{}", pf.dump_text());
}

/// Control: `name` and `message` are ToString'd, so a message object's own
/// `toString` runs, and what it hands to unknown code escapes.
#[test]
fn error_tostring_runs_the_message_tostring() {
    let pf = whole(&src(
        "const kept = { keptmark: 1 };
         const o = { name: 'N', message: { toString() { unknown.keep(kept); return 'm'; } } };
         console.log(Error.prototype.toString.call(o), e);",
    ));
    assert!(escape_with_field(&pf, "keptmark").is_some(), "{}", pf.dump_text());
}

/// Control: a `name` getter runs with `this` = the receiver.
#[test]
fn error_tostring_runs_a_name_getter() {
    let pf = whole(&src(
        "const g = { getmark: 1 };
         const o = { get name() { unknown.keep(g); return 'N'; }, message: 'M' };
         console.log(Error.prototype.toString.call(o), e);",
    ));
    assert!(escape_with_field(&pf, "getmark").is_some(), "{}", pf.dump_text());
}

/// Control: the instance really handed to unknown code escapes.
#[test]
fn an_error_instance_given_to_unknown_code_escapes() {
    let pf = whole(&src("new Function(process.argv[5])()(e);"));
    assert!(escape_with_field(&pf, "response").is_some(), "{}", pf.dump_text());
}
