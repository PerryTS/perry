//! Rule TA-LEN (`Model::ta_fixed_len`): a computed read of a typed array
//! born with a known length (`new TA(k)`, `new TA([e1, .., en])`) under an
//! integer key below that length is an element, never `undefined`
//! (ECMA-262 10.4.5.4, 10.4.5.15), while nothing may detach its buffer or
//! change how its source literal iterates. Observed through the elements of
//! the program Array the read fills (`Rep::Arr` elem F64 vs Boxed).

use crate::bindings::{ElemRep, Rep};
use crate::FactsConfig;

fn elem(body: &str) -> ElemRep {
    let src = format!(
        "function fill(dst: any, src: any, off: number) {{\n\
           for (let i = 0; i < 4; i++) dst[i] = src[i + off];\n\
         }}\n\
         const a = [0, 0, 0, 0];\n\
         {body}\n\
         console.log(a[0] + a[3]);"
    );
    let r = super::array_reps::reps_with(&src, FactsConfig::default());
    match r.get("a") {
        Some(Rep::Arr(x)) => x.elem,
        other => panic!("no Arr rep for a: {other:?}"),
    }
}

#[test]
fn literal_source_in_bounds_is_a_number() {
    let e = elem("const ta = new Float32Array([1, 2, 3, 4, 5]); fill(a, ta, 1);");
    assert_eq!(e, ElemRep::F64);
}

#[test]
fn length_source_in_bounds_is_a_number() {
    let e = elem("const ta = new Float64Array(4); ta[2] = 0.5; fill(a, ta, 0);");
    assert_eq!(e, ElemRep::F64);
}

#[test]
fn a_key_at_the_length_reads_undefined() {
    let e = elem("const ta = new Float32Array([1, 2, 3, 4]); fill(a, ta, 1);");
    assert_eq!(e, ElemRep::Boxed);
}

#[test]
fn a_negative_key_reads_undefined() {
    let e = elem("const ta = new Float32Array([1, 2, 3, 4]); fill(a, ta, -1);");
    assert_eq!(e, ElemRep::Boxed);
}

#[test]
fn an_exposed_buffer_may_be_detached() {
    let e = elem(
        "const ta = new Float32Array([1, 2, 3, 4]); (globalThis as any).hostKeep(ta.buffer); fill(a, ta, 0);",
    );
    assert_eq!(e, ElemRep::Boxed);
}

#[test]
fn a_transfer_through_another_view_detaches_the_buffer() {
    let e = elem(
        "const ta = new Float32Array([1, 2, 3, 4]); const sub: any = ta.subarray(1); fill(a, ta, 0); (sub.buffer as any).transfer();",
    );
    assert_eq!(e, ElemRep::Boxed);
}

#[test]
fn an_escaped_typed_array_may_be_detached() {
    let e = elem("declare function external(x: unknown): void; const ta = new Float32Array(4); external(ta); fill(a, ta, 0);");
    assert_eq!(e, ElemRep::Boxed);
}

#[test]
fn a_replaced_array_iterator_may_list_other_elements() {
    let e = elem(
        "(Array.prototype as any)[Symbol.iterator] = function* () { yield 1; }; const ta = new Float32Array([1, 2, 3, 4]); fill(a, ta, 0);",
    );
    assert_eq!(e, ElemRep::Boxed);
}

#[test]
fn a_view_of_a_given_buffer_has_no_birth_length() {
    let e = elem("const b = new ArrayBuffer(16); const ta = new Float32Array(b); fill(a, ta, 0);");
    assert_eq!(e, ElemRep::Boxed);
}

/// three.js `Matrix4.fromArray(array, offset = 0)` (s14): the offset is a
/// default parameter the caller leaves out.
fn elem_from_array(call: &str) -> ElemRep {
    let src = format!(
        "class M {{\n\
           elements: any;\n\
           constructor() {{ this.elements = a; }}\n\
           fromArray(array: any, offset = 0) {{\n\
             for (let i = 0; i < 4; i++) this.elements[i] = array[i + offset];\n\
             return this;\n\
           }}\n\
         }}\n\
         const a = [0, 0, 0, 0];\n\
         const m = new M();\n\
         {call}\n\
         console.log(a[0] + a[3]);"
    );
    let r = super::array_reps::reps_with(&src, FactsConfig::default());
    match r.get("a") {
        Some(Rep::Arr(x)) => x.elem,
        other => panic!("no Arr rep for a: {other:?}"),
    }
}

#[test]
fn from_array_with_the_default_offset_is_a_number() {
    let e = elem_from_array("m.fromArray(new Float32Array([1, 0, 0, 0.5]));");
    assert_eq!(e, ElemRep::F64);
}

#[test]
fn from_array_past_the_end_reads_undefined() {
    let e = elem_from_array("m.fromArray(new Float32Array([1, 0, 0, 0.5]), 1);");
    assert_eq!(e, ElemRep::Boxed);
}
