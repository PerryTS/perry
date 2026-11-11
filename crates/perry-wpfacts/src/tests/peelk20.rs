//! PEELK20 (Blob members): skinbench GLTFLoader `loadBufferView`
//! (`return buffer.slice(byteOffset, byteOffset + byteLength)`, buffer a Blob when
//! FileLoader responseType is blob). W3C File API 3.3.3 slice(start, end, contentType):
//! start/end are WebIDL long long (Node: `start < 0`, Math.min: ToPrimitive), contentType
//! a DOMString (Node: template literal, ToString); the result is a new Blob. arrayBuffer(),
//! bytes(), text() return a promise of a fresh ArrayBuffer / Uint8Array / string, stream()
//! a fresh ReadableStream (3.3.4-3.3.7). Nothing else is handed to code.
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

/// `init` is the first `array`; two chained copies, then `tail` runs.
fn src(init: &str, tail: &str) -> String {
    format!(
        "const unknown: any = (globalThis as any)[process.argv[2]];
         const kept = {{ keptmark: 1 }};
         function copy(dst: any, source: any) {{ dst.array = new source.array.constructor(source.array); }}
         const a: any = {{ array: {init} }};
         const b: any = {{ array: null }};
         const c: any = {{ array: null }};
         copy(b, a);
         copy(c, b);
         {tail}
         console.log(c.array.length);"
    )
}

/// The copied value's `constructor` constructs `kept`.
const USE: &str = "try { const r: any = new c.array.constructor(kept); console.log(r.length); } catch (e) { console.log(\"t\"); }";

#[test]
fn blob_slice_coerces_only() {
    let pf = whole(
        "const kept: any = { keptmark: 1 };
         const ct: any = { ctmark: 1 };
         new Response(\"abc\").blob().then(function (b: any) {
             const s = b.slice(kept, kept + 1, ct);
             console.log(s.size);
         });",
    );
    assert_eq!(escape_with_field(&pf, "keptmark"), None, "{}", pf.dump_text());
    assert_eq!(escape_with_field(&pf, "ctmark"), None, "{}", pf.dump_text());
}

#[test]
fn blob_slice_result_is_blob() {
    let pf = whole(
        "const kept: any = { keptmark: 1 };
         new Blob([\"abc\"]).slice(0, 2).arrayBuffer().then(function (a: any) {
             const t = a.slice(kept, 1);
             console.log(t.byteLength);
         });",
    );
    assert_eq!(escape_with_field(&pf, "keptmark"), None, "{}", pf.dump_text());
}

#[test]
fn blob_text_and_stream() {
    let pf = whole(
        "const kept: any = { keptmark: 1 };
         new Response(\"abc\").blob().then(function (b: any) {
             b.text().then(function (s: any) { console.log(s.indexOf(kept)); });
             console.log(b.stream().locked);
         });",
    );
    assert_eq!(escape_with_field(&pf, "keptmark"), None, "{}", pf.dump_text());
}

#[test]
fn control_unknown_call_escapes() {
    let pf = whole(
        "const kept: any = { keptmark: 1 };
         const f: any = new Function(process.argv[2]);
         new Blob([\"abc\"]).slice(0, 1).text().then(function (s: any) { f(kept); });",
    );
    assert!(escape_with_field(&pf, "keptmark").is_some(), "{}", pf.dump_text());
}
