//! ECMA-262 23.1.3.28 `Array.prototype.slice` called with a receiver that is
//! no Array (a typed array, three.js `convertArray`): ArraySpeciesCreate
//! (10.4.2.3) step 3 makes a plain Array when `IsArray(this)` is false, and
//! every element is a [[Get]] of the receiver. The result is a known array:
//! nothing pushed into it reaches unknown code.
use super::*;

fn src(arg: &str, body: &str) -> String {
    format!(
        r#"
        class M {{ m = 2; go() {{ return this.m; }} }}
        function convertArray(array: any, type: any): any {{
            if (!array || array.constructor === type) return array;
            if (typeof type.BYTES_PER_ELEMENT === number) return new type(array);
            return Array.prototype.slice.call(array);
        }}
        const r: any = convertArray({arg}, Array);
        {body}
        r.push(new M());
        console.log(r[r.length - 1].go());
        "#
    )
}

fn escapes(pf: &ProgramFacts, name: &str) -> bool {
    tmpl(pf, name).escape.is_some()
}

#[test]
fn slice_call_of_typed_array_is_a_known_array() {
    let pf = facts(&src("new Float32Array(3)", ""));
    assert!(!escapes(&pf, "M"), "M escaped\n{}", pf.dump_text());
}

#[test]
fn slice_call_of_array_like_is_a_known_array() {
    let pf = facts(&src("{ 0: 1, length: 1 }", ""));
    assert!(!escapes(&pf, "M"), "M escaped\n{}", pf.dump_text());
}

/// Control: an unknown receiver keeps the result open.
#[test]
fn slice_call_of_unknown_stays_open() {
    let pf = facts(&src("new Function(process.argv[2])()", ""));
    assert!(escapes(&pf, "M"), "{}", pf.dump_text());
}

/// Control: the result handed to unknown code escapes what it holds.
#[test]
fn slice_result_handed_to_unknown_escapes() {
    let pf = facts(&src(
        "new Float32Array(3)",
        "const u: any = new Function(process.argv[2])(); u(r);",
    ));
    assert!(escapes(&pf, "M"), "{}", pf.dump_text());
}

/// The receiver made by a constructor picked from a table (GLTFLoader's
/// `new TypedArray(...)` with `TypedArray = WEBGL_COMPONENT_TYPES[type]`).
#[test]
fn slice_call_of_picked_typed_array_is_a_known_array() {
    let pf = facts(&src(
        "new ({ 5126: Float32Array, 5123: Uint16Array, 5121: Uint8Array } as any)[+process.argv[2]](3)",
        "",
    ));
    assert!(!escapes(&pf, "M"), "M escaped\n{}", pf.dump_text());
}

/// ECMA-262 23.1.3.28 step 1 ToObject(this) (7.1.18): a symbol or bigint
/// `this` is wrapped like a number; `length` of the wrapper reads its chain
/// (`Symbol.prototype`, `BigInt.prototype`, `Object.prototype`), and the
/// result is a fresh plain Array. skin root 20058 (BATCHB35): the solver
/// makes the call an unknown callee's (`native_this`, the share outside
/// `ARRAY_BOXED_PRIMS`) and its result ⊤. BATCHB36: the wrapper is modeled (`array_method_on_prim`).
#[test]
fn slice_call_of_symbol_is_a_known_array() {
    let pf = facts(&src("(process.argv.length > 9 ? Symbol() : [1])", ""));
    assert!(!escapes(&pf, "M"), "M escaped\n{}", pf.dump_text());
}

#[test]
fn slice_call_of_bigint_is_a_known_array() {
    let pf = facts(&src("(process.argv.length > 9 ? BigInt(1) : [1])", ""));
    assert!(!escapes(&pf, "M"), "M escaped\n{}", pf.dump_text());
}

/// Control: a receiver unknown code returns, without `convertArray`'s
/// guards, keeps the result open.
#[test]
fn direct_slice_call_of_unknown_stays_open() {
    let pf = facts(
        r#"
        class M { m = 2; go() { return this.m; } }
        const r: any = Array.prototype.slice.call(new Function(process.argv[2])());
        r.push(new M());
        console.log(r[r.length - 1].go());
        "#,
    );
    assert!(tmpl(&pf, "M").escape.is_some(), "{}", pf.dump_text());
}
