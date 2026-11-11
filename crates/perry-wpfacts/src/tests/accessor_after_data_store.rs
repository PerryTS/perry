//! A plain assignment `O[k] = v` is [[Set]] (ECMA-262 10.1.9.2
//! OrdinarySetWithOwnDescriptor): it creates or updates a DATA property, or
//! calls an existing setter; it never installs an accessor. So a store onto a
//! value that may be `Object.prototype` leaves an accessor descriptor query
//! under any key (`__lookupGetter__(k)`, the getter walk) with the accessors
//! the program defined. tssrc (PEELS10, seq 19895):
//! `Debug.attachDebugPrototypeIfDebug(m)` may be `Debug.__lookupGetter__(m)`
//! (debug.ts assertionCache), and the checker stores `type`, `symbol` ...
//! onto values that may be `Object.prototype`.
use super::annexb_known_accessor::{check, debug_cache_with};

/// A store onto a value that may be `Object.prototype`, under a named key.
const PROTO_STORE: &str =
    "const P: any = process.argv.length > 9 ? Object.getPrototypeOf({}) : {}; P.type = noop;";

fn walk(setup: &str) -> String {
    format!(
        r#"
        class Cell {{ witness = 1; }}
        const cell = new Cell();
        function noop(): void {{}}
        const o: any = {{}};
        {setup}
        const g = o.__lookupGetter__(process.argv[3]);
        g.k = cell;
        console.log(cell.witness);
    "#
    )
}

#[test]
fn walk_after_a_data_store_on_object_prototype_holds_no_top() {
    check(&walk(PROTO_STORE), true);
}

/// The tsc shape: the walk is `Debug.assertNode(..)` read as
/// `Debug.__lookupGetter__(..)`, after the checker's data stores.
#[test]
fn debug_walk_after_a_data_store_on_object_prototype_holds_no_top() {
    let src = debug_cache_with(
        "Marker",
        &format!("{PROTO_STORE} const r: any = Debug.assertNode(1, Marker); r.k = cell;"),
    );
    check(&src, true);
}

const PROTO_ACCESSOR: &str =
    "Object.defineProperty(Object.prototype, process.argv[2], { get() { return 1; } });";

/// Control: an accessor really installed on `Object.prototype` under an
/// unknown key: the walk is unknown.
#[test]
fn walk_after_an_accessor_defined_on_object_prototype_stays_unknown() {
    check(&walk(&format!("{PROTO_STORE} {PROTO_ACCESSOR}")), false);
}

/// Control: the same in the tsc shape.
#[test]
fn debug_walk_after_an_accessor_defined_on_object_prototype_stays_unknown() {
    let src = debug_cache_with(
        "Marker",
        &format!(
            "{PROTO_STORE} {PROTO_ACCESSOR} const r: any = Debug.assertNode(1, Marker); r.k = cell;"
        ),
    );
    check(&src, false);
}

/// Control: the holder handed to code nothing models may get any accessor.
#[test]
fn walk_after_a_data_store_on_an_escaped_holder_stays_unknown() {
    check(
        &walk(&format!("{PROTO_STORE} new Function(process.argv[4])(o);")),
        false,
    );
}
