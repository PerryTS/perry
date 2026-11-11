//! Iterating a value that may be a realm object (`Object.prototype`,
//! `Array.prototype`, a builtin function) beside a program object. GetIterator
//! (ECMA-262 7.4.3) makes the record's [[Iterator]] the result of
//! `Call(v[@@iterator], v)`: absent (Object.prototype, a function), it throws
//! TypeError and no `next` is read or called (7.3.14 Call step 2);
//! Array.prototype's is `values`, a fresh ArrayIterator (23.1.3.40). The
//! realm's own `next` methods throw for a receiver without iterator slots
//! (23.1.5.2.1, 27.5.3.3). So the realm value is never stepped as its own
//! iterator, and the program object iterated beside it is not handed to an
//! opaque builtin call.
//! tssrc (PEELS14, call 121730): a for-of over a value holding
//! Object.prototype, Array.prototype and object literals: its `next` read off
//! Object.prototype was the opaque builtin function, called with every
//! iterated value as `this`.
use super::annexb_known_accessor::check;

fn prog(realm: &str, use_: &str) -> String {
    format!(
        r#"
        class Cell {{ witness = 1; }}
        const cell = new Cell();
        const it: any = {{
            cell,
            [Symbol.iterator]() {{ return this; }},
            next() {{ return {{ done: true, value: 0 }}; }},
        }};
        const pick: any = process.argv.length > 9 ? {realm} : it;
        try {{ for (const x of pick) {{ console.log(x); }} }} catch (e) {{}}
        {use_}
        console.log((it.cell as Cell).witness);
    "#
    )
}

#[test]
fn object_prototype_iterated_beside_an_iterator_keeps_it() {
    check(&prog("Object.prototype", ""), true);
}

#[test]
fn array_prototype_iterated_beside_an_iterator_keeps_it() {
    check(&prog("Array.prototype", ""), true);
}

#[test]
fn builtin_function_iterated_beside_an_iterator_keeps_it() {
    check(&prog("Object", ""), true);
}

#[test]
fn value_handed_to_new_function_code_escapes() {
    check(
        &prog("Object.prototype", "new Function(process.argv[2])(it);"),
        false,
    );
}
