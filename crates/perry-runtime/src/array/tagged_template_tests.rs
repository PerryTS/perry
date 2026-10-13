//! The tagged-template template object: the cooked/raw pair a call site caches
//! must stay reachable through the `.raw` READ path.

use super::*;

/// The template object a tagged call receives answers `.raw` with the
/// raw-strings array registered for it.
///
/// `js_tagged_template_get_or_init` may GROW the cooked literal — installing the
/// first named property on a full array reallocates it — so the address the
/// caller passed and the address the pair must be keyed on can differ. The read
/// path resolves the receiver first (`js_template_raw` → `clean_arr_ptr`), so a
/// pair keyed on a pre-growth address is a permanent miss: `strings.raw` read
/// `undefined` while the own `raw` property still resolved (node-suite
/// `globals/tagged-template-cache-freeze`, #9202).
///
/// The pair is built exactly as `Expr::TaggedTemplateStrings` builds it, via
/// `js_array_from_values` — a LITERAL array is allocated with
/// `capacity == length`, which is what forces the grow.
#[test]
fn tagged_template_pair_resolves_through_the_read_path() {
    let _global = crate::gc::global_side_table_test_lock();
    let cooked = super::alloc::js_array_from_values([1.0, 2.0].as_ptr(), 2);
    let raw = super::alloc::js_array_from_values([3.0, 4.0].as_ptr(), 2);

    let template = js_tagged_template_get_or_init(9_202, cooked, raw);
    assert!(
        !template.is_null(),
        "get_or_init returns the template object"
    );
    assert_ne!(
        js_template_raw(template),
        0,
        "the template object the call receives must resolve `.raw`"
    );
    assert_ne!(
        js_template_raw(cooked),
        0,
        "the canonical cooked head must resolve `.raw`"
    );

    // The per-site cache replays the same object on every later evaluation, and
    // `.raw` must resolve for that one too.
    let again = js_tagged_template_get_or_init(9_202, cooked, raw);
    assert_eq!(again, template, "the site cache returns the same object");
    assert_ne!(js_template_raw(again), 0);
}
