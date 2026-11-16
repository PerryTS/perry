//! KEYCALL1: ToPropertyKey (ECMA-262 7.1.19) of an object key calls the
//! key's own `@@toPrimitive` / `toString` / `valueOf` (7.1.1). Every
//! operation of the program's syntax that converts a key goes through
//! `Gen::to_property_key`; a method only that conversion calls must be
//! reached, never inactive (a trap body).

use super::*;

/// The bodies the completed solve classified and no path reaches.
fn inactive(src: &str) -> usize {
    let pf = facts(src);
    assert!(pf.body_reach.complete, "{}", pf.dump_text());
    pf.body_reach
        .seen
        .iter()
        .filter(|id| pf.body_reach.inactive(id))
        .count()
}

const KEY: &str = "const k: any = { toString() { return process.argv[2] || 'a'; } };\n";

fn only_the_conversion_calls_it(op: &str) {
    let src = format!("{KEY}{op}\n");
    assert_eq!(inactive(&src), 0, "toString must be reached by: {op}");
}

/// The control: the key object is never converted, so its `toString` is
/// inactive (the test can fail).
#[test]
fn an_unconverted_key_object_leaves_its_tostring_inactive() {
    assert_eq!(inactive(&format!("{KEY}console.log(typeof k);\n")), 1);
}

/// ECMA-262 13.5.1.2 delete → 13.3.3 → ToPropertyKey. node: `1`.
#[test]
fn delete_of_a_computed_key_calls_its_tostring() {
    only_the_conversion_calls_it(
        "const o: any = { a: 1, b: 2 }; delete o[k]; console.log(Object.keys(o).length);",
    );
}

/// The result of `delete` is used. node: `true false`.
#[test]
fn delete_of_a_computed_key_as_a_value_calls_its_tostring() {
    only_the_conversion_calls_it(
        "const o: any = { a: 1 }; const r = delete o[k]; console.log(r, 'a' in o);",
    );
}

/// 15.4.4 ClassElementEvaluation → ComputedPropertyName. node: `4`.
#[test]
fn a_computed_class_method_name_calls_its_tostring() {
    only_the_conversion_calls_it(
        "class C { [k]() { return 4; } } const c: any = new C(); console.log(c.a());",
    );
}

/// 15.7.10 ClassFieldDefinitionEvaluation → ComputedPropertyName. node: `a`.
#[test]
fn a_computed_class_field_name_calls_its_tostring() {
    only_the_conversion_calls_it("class C { [k] = 9; } console.log(Object.keys(new C())[0]);");
}

/// The modelled operations stay modelled (read, store, update, `in`).
#[test]
fn read_store_update_and_in_call_its_tostring() {
    for op in [
        "const o: any = { a: 5 }; console.log(o[k]);",
        "const o: any = {}; o[k] = 7; console.log(Object.keys(o)[0]);",
        "const o: any = { a: 1 }; o[k]++; console.log(o.a);",
        "const o: any = { a: 1 }; console.log(k in o);",
    ] {
        only_the_conversion_calls_it(op);
    }
}

/// ECMA-262 23.1.3 (slice, splice, lastIndexOf, copyWithin, flat):
/// ToIntegerOrInfinity of the index, count and depth operands runs an
/// object operand's `valueOf` (7.1.1 hint number). node: `2 2 0 3,4,3,4 2`.
#[test]
fn array_index_operands_call_their_valueof() {
    let v = "const v: any = { valueOf() { return Number(process.argv[2] || 2); } };\n";
    for op in [
        "console.log([1, 2, 3, 4].slice(v).length);",
        "console.log([1, 2, 3, 4].splice(v).length);",
        "console.log([1, 2, 3, 4].lastIndexOf(1, v));",
        "console.log([1, 2, 3, 4].copyWithin(0, v).join());",
        "console.log([[1, [2]]].flat(v).length);",
    ] {
        assert_eq!(
            inactive(&format!("{v}{op}\n")),
            0,
            "valueOf must be reached by: {op}"
        );
    }
}

/// ECMA-262 23.1.3 / 23.2.3 / 23.1.2.1: the index, count and length operands of
/// these methods are converted (ToIntegerOrInfinity / ToLength: 7.1.5, 7.1.20 ->
/// ToNumber -> ToPrimitive hint number), so an object operand's `valueOf` runs.
/// One program per method; the control is `an_unconverted_key_object_leaves_its_tostring_inactive`.
#[test]
fn index_operands_of_the_runtime_converted_methods_call_their_valueof() {
    let v = "const v: any = { valueOf() { return Number(process.argv[2] || 1); } };\n\
             const a: any = [1, 2, 3, 4, 5]; const u = new Uint8Array([1, 2, 3, 4, 5]);\n\
             const f = new Float64Array([1, 2, 3, 4, 5]);\n";
    for op in [
        "console.log(a.at(v));",
        "console.log(a.with(v, 9).length);",
        "console.log(a.toSpliced(v, v).length);",
        "console.log(a.toSpliced(0, v, 7).length);",
        "console.log(u.subarray(v, 4).length);",
        "console.log(f.subarray(0, v).length);",
        "console.log(u.slice(v).length);",
        "console.log(u.at(v));",
        "console.log(f.at(v));",
        "console.log(u.with(v, 9).length);",
        "console.log(f.with(v, 9).length);",
        "console.log(u.copyWithin(v, 0).length);",
        "console.log(f.copyWithin(0, v).length);",
        "console.log(u.fill(7, v).length);",
        "console.log(f.fill(7, 0, v).length);",
        "console.log(Array.from({ length: v }).length);",
        "console.log(u.with(0, v).length);",
        "console.log(f.with(0, v).length);",
        "a.length = v; console.log(a.length);",
        "Object.defineProperty(a, 'length', { value: v }); console.log(a.length);",
        "console.log(Reflect.set(a, 'length', v));",
    ] {
        assert_eq!(
            inactive(&format!("{v}{op}\n")),
            0,
            "valueOf must be reached by: {op}"
        );
    }
}
