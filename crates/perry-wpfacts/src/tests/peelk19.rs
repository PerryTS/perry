//! PEELK19 (construct of a built-in constructor VALUE): skinbench three.core.js
//! `BufferAttribute.copy` (`this.array = new source.array.constructor(source.array)`),
//! copy of a copy. A built-in constructor reached as a value constructs exactly as
//! the syntactic `new` of it (ECMA-262 10.3.2 [[Construct]]): Number/String/Boolean
//! give a wrapper (21.1.1.1, 22.1.1.1, 20.3.1.1) whose `constructor` is known;
//! Symbol, BigInt and %TypedArray% throw a TypeError (20.4.1.1, 21.2.1.1, 23.2.1.1).
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
fn float32_copy_of_copy() {
    let pf = whole(&src("new Float32Array(4)", USE));
    assert_eq!(
        escape_with_field(&pf, "keptmark"),
        None,
        "{}",
        pf.dump_text()
    );
}

#[test]
fn mixed_typed_arrays() {
    let pf = whole(&src(
        "process.argv.length > 3 ? new Float32Array(4) : new Uint16Array(2)",
        USE,
    ));
    assert_eq!(
        escape_with_field(&pf, "keptmark"),
        None,
        "{}",
        pf.dump_text()
    );
}

#[test]
fn number_wrapper_copy_of_copy() {
    let pf = whole(&src("new Number(4)", USE));
    assert_eq!(
        escape_with_field(&pf, "keptmark"),
        None,
        "{}",
        pf.dump_text()
    );
}

#[test]
fn string_or_boolean_wrapper() {
    let pf = whole(&src(
        "process.argv.length > 3 ? new String(\"x\") : new Boolean(true)",
        USE,
    ));
    assert_eq!(
        escape_with_field(&pf, "keptmark"),
        None,
        "{}",
        pf.dump_text()
    );
}

#[test]
fn symbol_or_bigint_value_constructs_nothing() {
    let pf = whole(&src(
        "new Float32Array(1)",
        "const S: any = process.argv.length > 3 ? Symbol : BigInt;
         try { const r: any = new S(kept); unknown.keep(r); } catch (e) { console.log(\"t\"); }",
    ));
    assert_eq!(
        escape_with_field(&pf, "keptmark"),
        None,
        "{}",
        pf.dump_text()
    );
}

#[test]
fn control_new_function_value_stays_unknown() {
    let pf = whole(&src(
        "new Float32Array(1)",
        "const F: any = new Function(process.argv[2]);
         try { console.log(new F(kept)); } catch (e) { console.log(\"t\"); }",
    ));
    assert!(
        escape_with_field(&pf, "keptmark").is_some(),
        "{}",
        pf.dump_text()
    );
}

#[test]
fn control_program_class_value_constructs_class() {
    let pf = whole(&src(
        "new Float32Array(1)",
        "class K { constructor(x: any) { unknown.keep(x); } }
         const C: any = process.argv.length > 3 ? K : Float32Array;
         try { console.log(new C(kept)); } catch (e) { console.log(\"t\"); }",
    ));
    assert!(
        escape_with_field(&pf, "keptmark").is_some(),
        "{}",
        pf.dump_text()
    );
}

/// skinbench GLTFLoader `loadBufferView`: `buffer.slice(byteOffset, byteOffset + byteLength)`
/// on the ArrayBuffer `Body.arrayBuffer()` makes. ArrayBuffer.prototype.slice
/// (25.1.6.7) converts its operands ToIntegerOrInfinity and hands them to no code.
#[test]
fn realm_array_buffer_slice() {
    let pf = whole(
        "const kept: any = { keptmark: 1 };
         new Response(\"abc\").arrayBuffer().then(function (buffer: any) {
             const s = buffer.slice(kept, kept + 1);
             console.log(s.byteLength);
         });",
    );
    assert_eq!(
        escape_with_field(&pf, "keptmark"),
        None,
        "{}",
        pf.dump_text()
    );
}
