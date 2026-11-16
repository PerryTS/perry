//! The Web Fetch `Response` body (three.js FileLoader:
//! `const stream = new ReadableStream({ start(controller) { .. controller.enqueue(value) .. } });
//! return new Response(stream)`, then `.arrayBuffer()` / `.json()` / `.text()`).
//! Node (undici `extractBody`, webidl `BodyInit`): a ReadableStream body is
//! kept and read through the stream's own protocol (`locked`, later its
//! reader), which reaches only the source's `start`/`pull`/`cancel`; a
//! string, buffer source, Blob or FormData is read and nothing is called;
//! URLSearchParams is read by its `toString`; any other object with a
//! `[Symbol.asyncIterator]` method is iterated (`ReadableStreamFrom`: the
//! method, then `next()`/`return()` on what it returns, each result's
//! `then`, then `done`/`value`, and `Buffer.from(value)`); anything else is
//! ToString'd. The body is never handed to anyone else.
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

fn src(body: &str) -> String {
    format!(
        "const unknown: any = (globalThis as any)[process.argv[2]];
         class A {{ a = 1; }}
         const a = new A();
         fetch(process.argv[3]).then((response: any) => {{
           {body}
         }}).then((r: any) => r.arrayBuffer()).then((b: any) => console.log(b.byteLength, a.a));"
    )
}

/// FileLoader's progress stream: the stream and its source stay home.
#[test]
fn a_readable_stream_body_hands_nothing_to_unknown_code() {
    let pf = whole(&src(
        "const reader = response.body.getReader();
         const source = { srcmark: 1, start(controller: any) {
           function readData() {
             reader.read().then(({ done, value }: any) => {
               if (done) { controller.close(); } else { controller.enqueue(value); readData(); }
             });
           }
           readData();
           console.log(a.a);
         } };
         const stream = new ReadableStream(source as any);
         return new Response(stream);",
    ));
    assert_eq!(escape_with_field(&pf, "srcmark"), None, "{}", pf.dump_text());
    assert_eq!(escape_with_field(&pf, "a"), None, "{}", pf.dump_text());
}

/// A string body and an object body read by ToString hand nothing out.
#[test]
fn a_plain_body_hands_nothing_to_unknown_code() {
    let pf = whole(&src(
        "const b = { plainmark: 1, toString() { return String(this.plainmark); } };
         new Response(\x27x\x27);
         return new Response(b as any);",
    ));
    assert_eq!(escape_with_field(&pf, "plainmark"), None, "{}", pf.dump_text());
}

/// An async iterable body whose methods keep to themselves hands nothing out.
#[test]
fn an_async_iterable_body_hands_nothing_to_unknown_code() {
    let pf = whole(&src(
        "const b = { itmark: 1, [Symbol.asyncIterator]() {
           return { next() { return Promise.resolve({ done: true, value: undefined }); } };
         } };
         return new Response(b as any);",
    ));
    assert_eq!(escape_with_field(&pf, "itmark"), None, "{}", pf.dump_text());
}

/// Control: the body's `[Symbol.asyncIterator]` runs with `this` = the body.
#[test]
fn an_async_iterable_body_runs_its_iterator_method_on_the_body() {
    let pf = whole(&src(
        "const b = { selfmark: 1, [Symbol.asyncIterator]() {
           unknown.keep(this);
           return { next() { return Promise.resolve({ done: true, value: undefined }); } };
         } };
         return new Response(b as any);",
    ));
    assert!(escape_with_field(&pf, "selfmark").is_some(), "{}", pf.dump_text());
}

/// Control: `next` runs on what `[Symbol.asyncIterator]` returned, and the
/// chunk it settles with is read by `Buffer.from` (its `valueOf` runs).
#[test]
fn an_async_iterable_body_steps_its_iterator_and_reads_its_chunks() {
    let pf = whole(&src(
        "const chunk = { chunkmark: 1, valueOf() { unknown.keep(this); return 1; } };
         const b = { [Symbol.asyncIterator]() {
           return { itermark: 1, next() { unknown.keep(this); return Promise.resolve({ done: false, value: chunk }); } };
         } };
         return new Response(b as any);",
    ));
    assert!(escape_with_field(&pf, "itermark").is_some(), "{}", pf.dump_text());
    assert!(escape_with_field(&pf, "chunkmark").is_some(), "{}", pf.dump_text());
}

/// Control: an object body's `toString` runs with `this` = the body.
#[test]
fn a_plain_body_runs_its_to_string_on_the_body() {
    let pf = whole(&src(
        "const b = { strmark: 1, toString() { unknown.keep(this); return \x27x\x27; } };
         return new Response(b as any);",
    ));
    assert!(escape_with_field(&pf, "strmark").is_some(), "{}", pf.dump_text());
}

/// Control: a body handed to unknown code still escapes.
#[test]
fn a_body_handed_to_unknown_code_escapes() {
    let pf = whole(&src(
        "const b = { keptmark: 1 };
         unknown.keep(b);
         return new Response(b as any);",
    ));
    assert!(escape_with_field(&pf, "keptmark").is_some(), "{}", pf.dump_text());
}
