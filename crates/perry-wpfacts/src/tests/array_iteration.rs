//! #13645: iterating an array reads `Array.prototype[@@iterator]`, the
//! array iterator's `next`, and (on early exit) its `return`. When no
//! compiled code writes any of them, both program facts are `None` and
//! codegen emits nothing for them. Each negative differs from the positive
//! by one write.

use super::*;

const LOOP: &str = "
    const xs: number[] = [];
    for (let i = 0; i < 4; i += 1) xs[i] = i;
    xs[5] = 1;
    let sum = 0;
    for (const x of xs) sum += x;
    console.log(sum);
";

/// Numeric-key stores are elements, never members.
#[test]
fn numeric_key_writes_leave_both_iteration_facts_proven() {
    let pf = facts(LOOP);
    assert!(pf.array_iteration_written.is_none(), "{:?}", pf.array_iteration_written);
    assert!(pf.array_iterator_close_written.is_none(), "{:?}", pf.array_iterator_close_written);
}

/// A replaced `@@iterator` changes what the loop iterates.
#[test]
fn a_replaced_array_iterator_method_is_written() {
    let pf = facts(&format!(
        "{LOOP}
        (Array.prototype as any)[Symbol.iterator] = function () {{ return [7][Symbol.iterator](); }};"
    ));
    assert!(pf.array_iteration_written.is_some());
}

/// An early exit would call the new `return`.
#[test]
fn a_return_on_the_array_iterator_chain_is_written() {
    let pf = facts(&format!(
        "{LOOP}
        const proto = Object.getPrototypeOf([][Symbol.iterator]());
        proto.return = function () {{ return {{}}; }};"
    ));
    assert!(pf.array_iterator_close_written.is_some());
}

/// `%IteratorPrototype%` is on the array iterator's chain.
#[test]
fn a_next_on_the_iterator_prototype_chain_is_written() {
    let pf = facts(&format!(
        "{LOOP}
        const ip = Object.getPrototypeOf(Object.getPrototypeOf([][Symbol.iterator]()));
        ip.next = function () {{ return {{ done: true }}; }};"
    ));
    assert!(pf.array_iterator_close_written.is_some());
}
