//! `Object.defineProperty(O, P, Desc)` installs exactly what
//! ToPropertyDescriptor(Desc) says (ECMA-262 6.2.6.5; 10.1.6.3
//! ValidateAndApplyPropertyDescriptor): a descriptor with `value` and no
//! get/set makes a DATA property, one with `get`/`set` installs exactly those
//! functions. So a definition through an unknown receiver (which may be
//! `Object.prototype`) answers an accessor descriptor query under any key
//! (`__lookupGetter__(k)`, the getter walk) with the descriptor's own get/set
//! functions, not with an unknown value, as long as the descriptor is known.
//! tssrc (PEELS11, seq 19895): debug.ts `Object.defineProperties(<⊤>.prototype,
//! { __tsDebuggerDisplay: { value() {..} }, __debugFlowFlags: { get() {..} } })`
//! and `Debug.attachDebugPrototypeIfDebug(m)` read as `Debug.__lookupGetter__(m)`.
//!
//! The witness stores `cell` into the walk's result `g`: a store into ⊤ hands
//! it to unknown code, one into a builtin function (the realm's own
//! accessors, which an any-key walk also finds) does not. A program getter
//! installed through ⊤ is itself escaped (unknown code may read the key), so
//! a store into it escapes `cell` as well, and a call of a realm accessor
//! hands its arguments to builtin code: under any key no site observes the
//! getter (PEELS11 note: the realm accessors and the escaped getter decide).
//! The named-key fixtures call the program getter; a walk on a built-in
//! object that unknown code holds stays unknown by its own any-key node
//! (`accessor descriptor of a changed holder`), as it must.
use super::annexb_known_accessor::check;

/// Unknown code holds the global object (as in tssrc): a ⊤ value may then be
/// any built-in object, `Object.prototype` included.
const GLOBAL: &str = "unknown(globalThis);";

const DEFS: &str = "Object.defineProperties(unknown().prototype, { a: { value() { return 0; } }, b: { get() { return 1; } } });";

const ANY: &str = "process.argv[3]";
const STORE: &str = "g.k = cell;";

fn walk_with(setup: &str, key: &str, use_: &str) -> String {
    format!(
        r#"
        declare function unknown(): any;
        class Cell {{ witness = 1; }}
        const cell = new Cell();
        const o: any = {{}};
        {GLOBAL}
        {setup}
        const g = o.__lookupGetter__({key});
        {use_}
        console.log(cell.witness);
    "#
    )
}

/// Under the named key `b`: the program getter, called with `cell`.
fn walk(setup: &str) -> String {
    walk_with(setup, "\"b\"", "g(cell);")
}

/// Baseline of the any-key walk: the realm's own accessors only.
#[test]
fn any_key_walk_without_a_definition_through_unknown_holds_no_top() {
    check(&walk_with("", ANY, STORE), true);
}

/// Control: an unknown getter through ⊤ under the key: unknown.
#[test]
fn builtin_walk_after_an_unknown_getter_through_unknown_stays_unknown() {
    check(
        &walk_with(
            "Object.defineProperty(unknown(), \"b\", { get: unknown() });",
            "\"b\"",
            "",
        )
        .replace(
            "const g = o.__lookupGetter__(\"b\");",
            "const g = (Math as any).__lookupGetter__(\"b\"); g(cell);",
        ),
        false,
    );
}

#[test]
fn walk_without_a_definition_through_unknown_holds_no_top() {
    check(&walk(""), true);
}

#[test]
fn walk_after_value_and_get_definitions_through_unknown_holds_no_top() {
    check(&walk(DEFS), true);
}

#[test]
fn walk_after_a_getter_defined_on_unknown_holds_no_top() {
    check(
        &walk("Object.defineProperty(unknown(), \"b\", { get() { return 1; } });"),
        true,
    );
}

/// Under the data key `a`: `undefined`, the function is no getter.
#[test]
fn walk_of_a_value_definition_through_unknown_finds_no_getter() {
    check(&walk_with(DEFS, "\"a\"", "g(cell);"), true);
}

/// Control: the descriptor itself is unknown: it may name any getter.
#[test]
fn walk_after_an_unknown_descriptor_through_unknown_stays_unknown() {
    check(
        &walk_with(
            "Object.defineProperty(unknown(), \"b\", unknown());",
            ANY,
            STORE,
        ),
        false,
    );
}

/// Control: a known descriptor whose getter is an unknown function.
#[test]
fn walk_after_an_unknown_getter_through_unknown_stays_unknown() {
    check(
        &walk_with(
            "Object.defineProperty(unknown(), \"b\", { get: unknown() });",
            ANY,
            STORE,
        ),
        false,
    );
}

/// Control: a program getter installed through ⊤ is escaped; a store into
/// it reaches unknown code.
#[test]
fn any_key_walk_store_into_a_program_getter_through_unknown_stays_unknown() {
    check(&walk_with(DEFS, ANY, STORE), false);
}

/// Control: the holder handed to code nothing models may get any accessor.
#[test]
fn walk_after_definitions_on_an_escaped_holder_stays_unknown() {
    check(
        &walk_with(
            &format!("{DEFS} new Function(process.argv[4])(o);"),
            ANY,
            STORE,
        ),
        false,
    );
}
