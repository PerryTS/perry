//! A call of one of the realm's own accessor functions (`%RealmAccessor%`,
//! which an any-key `__lookupGetter__`/`__lookupSetter__` hands out, B.2.2.4/5)
//! runs built-in steps only: the getters read internal slots of `this` or do
//! `this.[[GetPrototypeOf]]()` (20.1.3.8.1), `RegExp.prototype.flags` does
//! [[Get]] of the flag keys on `this` (22.2.6.4), the @@species getters return
//! `this`; the setters take one argument (`__proto__` 20.1.3.8.2:
//! `this.[[SetPrototypeOf]](proto)`). A wrong receiver throws a TypeError.
//! None of them hands `this` or an argument to code it does not name.
//! tssrc (PEELS12, seq 19895): checker.ts `targets[i]()` with `mapper` read as
//! `Debug.__lookupGetter__(m)`: the array `targets` is the receiver of a call
//! whose callee may be a realm accessor.
//! The receiver `o` is a Map: `Map.prototype.size` is the realm accessor its
//! chain holds under an unknown string key (`Object.prototype` holds only
//! `__proto__`, which such a key never names: DECISIONS 217).
use super::annexb_known_accessor::check;

const ANY: &str = "process.argv[3]";

fn prog(setup: &str, use_: &str) -> String {
    format!(
        r#"
        declare function unknown(): any;
        class Cell {{ witness = 1; }}
        const cell = new Cell();
        const o: any = new Map();
        {setup}
        const g = o.__lookupGetter__({ANY});
        {use_}
        console.log(cell.witness);
    "#
    )
}

/// `h.g(..)` with `h` holding the accessor: `h` is the receiver.
#[test]
fn realm_accessor_called_as_a_method_keeps_its_receiver() {
    check(
        &prog("", "const h: any = { g, cell }; h.g();").replace(
            "console.log(cell.witness);",
            "console.log((h.cell as Cell).witness);",
        ),
        true,
    );
}

#[test]
fn realm_accessor_call_through_function_call_keeps_the_receiver() {
    check(&prog("", "g.call(cell);"), true);
}

#[test]
fn realm_accessor_call_keeps_its_argument() {
    check(&prog("", "g(cell);"), true);
}

/// The tssrc shape: an array holding the accessor is the receiver of the
/// call of its element.
#[test]
fn array_receiver_of_a_realm_accessor_element_call_is_kept() {
    check(
        &prog(
            "",
            "const targets: any = [g]; targets.cell = cell; for (let i = 0; i < targets.length; i++) targets[i]();",
        )
        .replace("console.log(cell.witness);", "console.log((targets.cell as Cell).witness);"),
        true,
    );
}

/// Control: a program getter installed under the unknown key with an
/// unknown get function: the lookup may return it, its call is unknown code.
#[test]
fn unknown_getter_defined_under_the_key_escapes_the_receiver() {
    check(
        &prog(
            &format!("Object.defineProperty(Object.prototype, {ANY}, {{ get: unknown() }});"),
            "g.call(cell);",
        ),
        false,
    );
}

/// Control: a value handed to dynamic code escapes.
#[test]
fn value_handed_to_new_function_code_escapes() {
    check(
        &prog("", "g.call(cell); new Function(process.argv[2])(cell);"),
        false,
    );
}

/// The result of `g.call(p)` is no property value of `p`: no realm getter
/// returns one (`flags` only applies ToBoolean to what it reads, 22.2.6.4).
/// `p.f` is then called by no code, so the dynamic code in it never runs.
#[test]
fn realm_accessor_result_is_no_field_value_of_this() {
    check(
        &prog(
            "const p: any = { f() { new Function(process.argv[2])(cell); } };",
            "const r = g.call(p); r();",
        ),
        true,
    );
}

/// B.2.2.4 on the chain of `recv`, key `key`; the result, called, is stored
/// into: a ⊤ result (a realm accessor's host value) escapes `cell`.
fn lookup_prog(setup: &str, recv: &str, key: &str) -> String {
    format!(
        r#"
        declare function unknown(): any;
        class Cell {{ witness = 1; }}
        const cell = new Cell();
        {setup}
        const g = ({recv} as any).__lookupGetter__({key});
        const r: any = g ? g.call({{}}) : {{}};
        r.c = cell;
        console.log(cell.witness);
    "#
    )
}

/// PEELS15 (tssrc `Debug.__lookupGetter__(m)`): a plain object's chain is
/// `Object.prototype`, whose one accessor `__proto__` an unknown string key
/// never names: the query answers `undefined`, nothing is called.
#[test]
fn unknown_key_on_a_plain_object_names_no_realm_accessor() {
    check(&lookup_prog("const o: any = { a: 1 };", "o", ANY), true);
}

/// The same on a namespace object (debug.ts `Debug`).
#[test]
fn unknown_key_on_a_namespace_names_no_realm_accessor() {
    check(
        &lookup_prog("namespace NS { export const a = 1; }", "NS", ANY),
        true,
    );
}

/// `Map.prototype.size` is on a Map's chain: the query may return it.
#[test]
fn unknown_key_on_a_map_may_name_its_size_getter() {
    check(&lookup_prog("const o = new Map();", "o", ANY), false);
}

/// A constant `"__proto__"` names Object.prototype's accessor (20.1.3.8).
#[test]
fn proto_key_names_the_proto_accessor() {
    check(
        &lookup_prog("const o: any = { a: 1 };", "o", "\"__proto__\""),
        false,
    );
}

/// A program getter under an unknown key is what the query returns.
#[test]
fn program_getter_under_an_unknown_key_is_returned() {
    check(
        &format!(
            r#"
        class Cell {{ witness = 1; }}
        const cell = new Cell();
        const box = {{ cell }};
        const o: any = {{}};
        Object.defineProperty(o, {ANY}, {{ get() {{ return box; }} }});
        const g = o.__lookupGetter__({ANY});
        const r: any = g ? g.call(o) : box;
        console.log((r.cell as Cell).witness);
    "#
        ),
        true,
    );
}

/// Control: a getter dynamic code installs on the plain object's chain.
#[test]
fn unknown_key_after_new_function_code_stays_unknown() {
    check(
        &lookup_prog(
            "const o: any = { a: 1 }; new Function(process.argv[2])(o);",
            "o",
            ANY,
        ),
        false,
    );
}
