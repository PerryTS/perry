//! #10894: `Uint8Array<ArrayBuffer>` and friends lower to the same
//! `Type::Named` as the bare class name.
//!
//! TypeScript 5.7 made every typed array and `DataView` generic over the
//! backing buffer, and `@types/node` did the same for `Buffer`. The argument
//! has no runtime meaning, but every typed-array / buffer recognizer in
//! lowering and codegen keys on `Type::Named("Uint8Array")`. Left as
//! `Type::Generic { base: "Uint8Array" }` the receiver matched none of them,
//! read as "known not a string", and the Array fast path folded
//! `m.indexOf(v)` to `Expr::ArrayIndexOf` over a `BufferHeader`: a silent -1.
//!
//! Two kinds of assertion, because the second is the one that can catch a
//! recognizer nobody thought to list:
//!
//! * the declared TYPE is `Named(base)` in every annotation position, and
//! * the lowered BODY of a function is *identical* whether its parameter is
//!   spelled `Uint8Array` or `Uint8Array<ArrayBuffer>` — for every method in
//!   the audit list. Any recognizer that still tells the two spellings apart
//!   shows up as a body diff, whatever it is.

#![cfg(test)]

use crate::lower_module;
use crate::types::Type;
use crate::Module;
use perry_diagnostics::SourceCache;
use perry_parser::parse_typescript_with_cache;

fn lower_src(src: &str) -> Module {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut cache = SourceCache::new();
            let parsed = parse_typescript_with_cache(&src, "test.ts", &mut cache)
                .expect("parse should succeed");
            lower_module(&parsed.module, "test", "test.ts").expect("lowering should succeed")
        })
        .expect("spawn")
        .join()
        .expect("lowering thread")
}

fn func<'m>(module: &'m Module, name: &str) -> &'m crate::ir::Function {
    module
        .functions
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("function {name} not lowered"))
}

fn named(name: &str) -> Type {
    Type::Named(name.to_string())
}

const BUFFER_BACKED: &[&str] = &[
    "Int8Array",
    "Uint8Array",
    "Uint8ClampedArray",
    "Int16Array",
    "Uint16Array",
    "Int32Array",
    "Uint32Array",
    "Float16Array",
    "Float32Array",
    "Float64Array",
    "BigInt64Array",
    "BigUint64Array",
    "Buffer",
    "DataView",
];

#[test]
fn every_buffer_backed_builtin_erases_its_backing_buffer_argument() {
    for base in BUFFER_BACKED {
        for arg in ["ArrayBuffer", "ArrayBufferLike", "SharedArrayBuffer"] {
            let module = lower_src(&format!(
                "export function f(m: {base}<{arg}>): {base}<{arg}> {{ return m; }}"
            ));
            let f = func(&module, "f");
            assert_eq!(f.params[0].ty, named(base), "param {base}<{arg}>");
            assert_eq!(f.return_type, named(base), "return {base}<{arg}>");
        }
    }
}

#[test]
fn the_erasure_holds_in_every_annotation_position() {
    let module = lower_src(
        r#"
type Bytes = Uint8Array<ArrayBuffer>;
export function alias(m: Bytes): number { return m.length; }
export function union(m: Uint8Array<ArrayBuffer> | null): number { return m ? m.length : 0; }
export function element(m: Uint8Array<ArrayBuffer>[]): number { return m.length; }
export function nested(m: Promise<Uint8Array<ArrayBuffer>>): number { return 0; }
export function arg(m: Map<string, Float64Array<ArrayBufferLike>>): number { return m.size; }
"#,
    );
    assert_eq!(func(&module, "alias").params[0].ty, named("Uint8Array"));
    assert_eq!(
        func(&module, "union").params[0].ty,
        Type::Union(vec![named("Uint8Array"), Type::Null])
    );
    assert_eq!(
        func(&module, "element").params[0].ty,
        Type::Array(Box::new(named("Uint8Array")))
    );
    assert_eq!(
        func(&module, "nested").params[0].ty,
        Type::Promise(Box::new(named("Uint8Array")))
    );
    assert_eq!(
        func(&module, "arg").params[0].ty,
        Type::Generic {
            base: "Map".to_string(),
            type_args: vec![Type::String, named("Float64Array")],
        }
    );
}

#[test]
fn a_user_class_of_the_same_name_keeps_its_type_arguments() {
    // `class Buffer<T>` is a plausible user type (a ring buffer). Its
    // arguments are real — monomorphization keys a specialization on them —
    // so the erasure must not claim it.
    let module = lower_src(
        r#"
class Buffer<T> {
  items: T[] = [];
  push(v: T): void { this.items.push(v); }
}
export function f(b: Buffer<number>): void { b.push(1); }
"#,
    );
    assert_eq!(
        func(&module, "f").params[0].ty,
        Type::Generic {
            base: "Buffer".to_string(),
            type_args: vec![Type::Number],
        }
    );
}

/// Lower `function f(m: <annotation>) { <body> }` and render the body.
///
/// The annotation is space-padded to a fixed width so the body sits at the
/// same byte offset under every spelling — HIR nodes carry source offsets, and
/// without the padding every comparison below would differ in nothing else.
fn body_under(annotation: &str, body: &str) -> String {
    let module = lower_src(&format!(
        "export function f(m: {annotation:<48}): unknown {{ {body} }}"
    ));
    format!("{:#?}", func(&module, "f").body)
}

#[test]
fn a_method_call_lowers_identically_under_both_spellings() {
    // One statement per method so a failure names the method. Every entry is
    // a `%TypedArray%.prototype` (or `Array.prototype`-named) member whose
    // lowering consults the receiver's declared type somewhere.
    let calls = [
        "return m.indexOf(44);",
        "return m.indexOf(44, 2);",
        "return m.lastIndexOf(44);",
        "return m.lastIndexOf(44, 2);",
        "return m.includes(44);",
        "return m.includes(44, 2);",
        "return m.at(-1);",
        "return m.slice(1, 3);",
        "return m.subarray(1);",
        "return m.join('-');",
        "return m.toString();",
        "return m.find((v) => v > 1);",
        "return m.findIndex((v) => v > 1);",
        "return m.findLast((v) => v > 1);",
        "return m.findLastIndex((v) => v > 1);",
        "return m.map((v) => v + 1);",
        "return m.filter((v) => v > 1);",
        "return m.reduce((a, v) => a + v, 0);",
        "return m.reduceRight((a, v) => a + v, 0);",
        "return m.some((v) => v > 1);",
        "return m.every((v) => v > 1);",
        "m.forEach((v) => { v; }); return 0;",
        "return m.fill(7, 1, 3);",
        "m.set([1, 2], 4); return 0;",
        "return m.copyWithin(0, 3);",
        "return m.reverse();",
        "return m.sort();",
        "return m.toReversed();",
        "return m.toSorted();",
        "return m.with(0, 9);",
        "return m.entries();",
        "return m.keys();",
        "return m.values();",
        "return [...m];",
        "let s = 0; for (const v of m) s += v; return s;",
        "return m[2];",
        "m[2] = 9; return 0;",
        "return m.length;",
        "return m.byteLength + m.byteOffset;",
        "return m.buffer;",
        "return m instanceof Uint8Array;",
    ];
    for call in calls {
        let bare = body_under("Uint8Array", call);
        for generic in ["Uint8Array<ArrayBuffer>", "Uint8Array<ArrayBufferLike>"] {
            assert_eq!(
                body_under(generic, call),
                bare,
                "`{call}` lowers differently under `{generic}` than under `Uint8Array`"
            );
        }
    }
    // The fold this issue was filed for, named outright: not an array search.
    assert!(
        !body_under("Uint8Array<ArrayBuffer>", "return m.indexOf(44, 2);").contains("ArrayIndexOf"),
        "a Uint8Array<ArrayBuffer> receiver must not fold to Expr::ArrayIndexOf"
    );
}

#[test]
fn the_other_kinds_lower_identically_under_both_spellings() {
    let calls = [
        "return m.indexOf(44, 2);",
        "return m.lastIndexOf(44);",
        "return m.includes(44);",
        "return m.map((v) => v);",
        "return m.sort();",
        "return m[1];",
    ];
    for base in [
        "Int8Array",
        "Uint8ClampedArray",
        "Uint16Array",
        "Int32Array",
        "Float32Array",
        "Float64Array",
    ] {
        for call in calls {
            assert_eq!(
                body_under(&format!("{base}<ArrayBuffer>"), call),
                body_under(base, call),
                "`{call}` lowers differently under `{base}<ArrayBuffer>` than under `{base}`"
            );
        }
    }
}

#[test]
fn new_with_type_arguments_infers_the_bare_class() {
    // `const u = new Uint8Array<ArrayBuffer>(8)` types `u` by inference, not
    // annotation — a second producer of the generic spelling.
    let with_args = lower_src(
        "export function f(): unknown { const u = new Uint8Array<ArrayBuffer>(8); return u.indexOf(44, 2); }",
    );
    let without = lower_src(
        "export function f(): unknown { const u = new Uint8Array(8); return u.indexOf(44, 2); }",
    );
    let render = |m: &Module| format!("{:#?}", func(m, "f").body);
    assert!(
        !render(&with_args).contains("ArrayIndexOf"),
        "new Uint8Array<ArrayBuffer>(n) must not type its binding as a plain Array"
    );
    // The `new` expression itself carries its type arguments, so compare the
    // RECEIVER's handling rather than the whole body.
    assert_eq!(
        render(&with_args).contains("ArrayIndexOf"),
        render(&without).contains("ArrayIndexOf")
    );
}
