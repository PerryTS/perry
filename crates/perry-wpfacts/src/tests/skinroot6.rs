//! SKINROOT6/7 (skin GLTFLoader.js:2341/2345, `for (const name in
//! objectDef.extensions) gltfExtensions[name] = objectDef.extensions[name]`,
//! then three.core.js:55849 `bindings[i].apply(accuIndex)` with receiver
//! GLTFLoader `json#2`): the JSON objects' prototype set holds ArrayBuffer
//! and typed-array instances (three.core.js `ArrayBuffer#85/#87`,
//! `indexed-owner@18/19`, `ArrayBuffer@call912`). An enumerable read at such
//! a link (`builtins::enum_free_link_obj`) finds what the program wrote on
//! it and, on a typed array, its elements: a built-in instance of these
//! kinds has no other enumerable own property, its prototype's members are
//! non-enumerable (clause 18), and a for-in key names an enumerable
//! property, first on the chain (14.7.5.9).
use super::*;

fn whole(src: &str) -> ProgramFacts {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    crate::compute(&[(path, &hir)], &FactsConfig::default())
}

fn field(pf: &ProgramFacts, key: &str) -> (Option<String>, crate::derive::KindFact) {
    let t = pf
        .templates
        .iter()
        .filter(|t| t.display.starts_with("t.ts"))
        .find(|t| t.fields.iter().any(|f| f.key == key))
        .unwrap_or_else(|| panic!("no t.ts template with field {key}\n{}", pf.dump_text()));
    let f = t.fields.iter().find(|f| f.key == key).unwrap();
    (t.escape.clone(), f.kind.clone())
}

fn escape_with_field(pf: &ProgramFacts, key: &str) -> Option<String> {
    field(pf, key).0
}

/// `e` = a parsed object whose prototype is `link` (`pre` may write on it;
/// without `set` the chain is Object.prototype only); `dst` (marked) is
/// filled by the same-object for-in read and then is the receiver of
/// `dst.foo(0)` (the c1257 shape: a builtin function callee on a non-buffer
/// receiver) and the argument of `dst.bar.call(unknown, dst)`; `r.keptv`
/// holds every value the read finds. A JSON value is not callable.
fn src_of(link: &str, pre: &str, set: bool) -> String {
    src_base(link, pre, set, "j.ext", "0")
}

fn src_base(link: &str, pre: &str, set: bool, base: &str, v0: &str) -> String {
    let sp = if set { "Object.setPrototypeOf(e, ab);" } else { "" };
    format!(
        "const unknown: any = (globalThis as any)[process.argv[2]];
         const j: any = JSON.parse(process.argv[3]);
         const e: any = {base};
         const ab: any = {link};
         {pre}
         {sp}
         const dst: any = {{ keptmark: 1 }};
         const r: any = {{ keptv: {v0} }};
         for (const name in e) {{ dst[name] = e[name]; r.keptv = e[name]; }}
         try {{ dst.foo(0); }} catch (x) {{ console.log(\"t\"); }}
         const g: any = dst.bar;
         try {{ g.call(unknown, dst); }} catch (x) {{ console.log(\"t\"); }}
         console.log(String(r.keptv), ab === unknown);"
    )
}

fn src(pre: &str) -> String {
    src_of("new ArrayBuffer(8)", pre, true)
}

fn green(link: &str) {
    let pf = whole(&src_of(link, "", true));
    assert_eq!(escape_with_field(&pf, "keptmark"), None, "{link}\n{}", pf.dump_text());
    let (_, k) = field(&pf, "keptv");
    assert!(!k.top && k.funcs == 0, "{link}: {k:?}\n{}", pf.dump_text());
}

/// Control: the same program without `Object.setPrototypeOf` (the chain is
/// Object.prototype): no builtin member, nothing escapes.
#[test]
fn control_skinroot6_no_set_prototype() {
    let pf = whole(&src_of("new ArrayBuffer(8)", "", false));
    assert_eq!(escape_with_field(&pf, "keptmark"), None, "{}", pf.dump_text());
}

/// The EnumChain link, an `ArrayBuffer` instance: its program-written
/// fields only; ArrayBuffer.prototype's methods do not reach dst.foo.
#[test]
fn green_skinroot6_arraybuffer_link() {
    let pf = whole(&src(""));
    assert_eq!(escape_with_field(&pf, "keptmark"), None, "{}", pf.dump_text());
}

/// Buffer family: ArrayBuffer, SharedArrayBuffer, DataView.
#[test]
fn green_skinroot7_buffer_links() {
    green("new ArrayBuffer(8)");
    green("new SharedArrayBuffer(8)");
    green("new DataView(new ArrayBuffer(8))");
}

/// A typed-array link: its elements, a Number (Uint8Array) or a BigInt
/// (BigInt64Array), reach the read; no %TypedArray%.prototype method.
#[test]
fn green_skinroot7_typed_array_link_elements() {
    green("new Uint8Array(2)");
    // `e` = { s: "x" }: the read finds "x" and, only through the link, the
    // element (NUM / BIGINT).
    let base = "{ s: \"x\" }";
    let pf = whole(&src_base("new Uint8Array(2)", "", true, base, "\"i\""));
    let (esc, k) = field(&pf, "keptv");
    assert!(k.prims & crate::model::prim::NUM != 0 && !k.top && k.funcs == 0, "{k:?}\n{}", pf.dump_text());
    assert_eq!(esc, None);
    let pf = whole(&src_base("new BigInt64Array(1)", "", true, base, "\"i\""));
    let (esc, k) = field(&pf, "keptv");
    assert!(k.prims & crate::model::prim::BIGINT != 0 && !k.top && k.funcs == 0, "{k:?}\n{}", pf.dump_text());
    assert_eq!(esc, None);
    // Control: without the link no Number reaches.
    let pf = whole(&src_base("new Uint8Array(2)", "", false, base, "\"i\""));
    assert!(field(&pf, "keptv").1.prims & crate::model::prim::NUM == 0, "{}", pf.dump_text());
}

/// Collection family: Map, Set, WeakMap, WeakSet, Promise.
#[test]
fn green_skinroot7_collection_links() {
    green("new Map()");
    green("new Set()");
    green("new WeakMap()");
    green("Promise.resolve(1)");
}

/// Stays: a program-written (enumerable) field on the ArrayBuffer link is
/// read: its function runs with this = dst and leaks it.
#[test]
fn keep_skinroot6_program_field_on_buffer_link() {
    let pf = whole(&src("ab.foo = function (this: any) { (globalThis as any).leak = this; };"));
    let k = field(&pf, "foo").1;
    assert!(k.funcs >= 1 || k.top, "{k:?}\n{}", pf.dump_text());
}

/// Stays: a program-added enumerable member of the built-in prototype
/// (`ArrayBuffer.prototype.foo = ...`) is found by the read.
#[test]
fn keep_skinroot7_program_member_on_builtin_proto() {
    let pf = whole(&src(
        "(ArrayBuffer.prototype as any).foo = function (this: any) { (globalThis as any).leak = this; };",
    ));
    let k = field(&pf, "foo").1;
    assert!(k.funcs >= 1 || k.top, "{k:?}\n{}", pf.dump_text());
}
