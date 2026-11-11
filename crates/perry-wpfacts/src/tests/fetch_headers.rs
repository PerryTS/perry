//! Web Fetch `Headers` (three.js FileLoader:
//! `response.headers.get(\x27X-File-Size\x27) || response.headers.get(\x27Content-Length\x27)`).
//! Node (WHATWG Fetch): every operand of `get`/`has`/`delete`/`set`/`append`
//! is a ByteString (ToString, no other hook), `get` answers a string or
//! null, `getSetCookie` a fresh array of strings, and `forEach` calls its
//! callback with (value, name, headers) and `this` = its second argument.
//! The constructor reads its init as a sequence of pairs or a record of
//! ByteStrings. Nothing else reaches program code; the headers object is
//! never handed to anyone.
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

fn src(headers: &str, body: &str) -> String {
    format!(
        "const unknown: any = (globalThis as any)[process.argv[2]];
         class A {{ a = 1; }}
         const a = new A();
         fetch(process.argv[3]).then((response: any) => {{
           const h: any = {headers};
           const n = h.get(\x27X-File-Size\x27) || h.get(\x27Content-Length\x27);
           h.has(a); h.get(a); h.set(a, a); h.append(a, a); h.delete(a);
           const c = h.getSetCookie();
           {body}
           console.log(n, c.length, a.a);
         }});"
    )
}

const QUIET: &str = "h.forEach(function (this: any, v: any, k: any, hh: any) { console.log(v, k, this.a); }, a);";

#[test]
fn file_loader_header_reads_hand_nothing_to_unknown_code() {
    let pf = whole(&src("response.headers", QUIET));
    assert_eq!(escape_with_field(&pf, "a"), None, "{}", pf.dump_text());
}

/// The constructor reads a record init by its own keys and ToString of
/// each value. (The calls are on `response.headers`: the HIR folds calls on
/// a `new Headers(..)` receiver into native module calls, a separate path.)
#[test]
fn a_headers_init_record_hands_nothing_to_unknown_code() {
    let pf = whole(&src("(new Headers(a), response.headers)", QUIET));
    assert_eq!(escape_with_field(&pf, "a"), None, "{}", pf.dump_text());
}

/// Control: the callback `forEach` runs with `this` = its second argument.
#[test]
fn headers_for_each_runs_the_callback_on_its_this_argument() {
    let pf = whole(&src(
        "response.headers",
        "h.forEach(function (this: any) { unknown.keep(this); }, a);",
    ));
    assert!(escape_with_field(&pf, "a").is_some(), "{}", pf.dump_text());
}
