//! Value-kind renderings the display ladders used to get wrong, plus the array
//! body layout that depends on them. (#9415.)
//!
//! Three defects, one mistake in three places: a ladder classifies a NaN-boxed
//! value by tag, has no arm for the case at hand, and lets the bits fall
//! through to "must be a regular number" (or to a hard-coded string). A class
//! reference printed as its raw class id (`49`); a sparse-array hole printed as
//! `NaN`, because `TAG_HOLE`'s bit pattern *is* a NaN; every promise printed as
//! `<pending>`. None of the three is a formatting preference — each is a value
//! decoded as the wrong kind.
//!
//! The renderings live here rather than in `formatting.rs` so the display
//! ladders across `console.rs` and `formatting.rs` share one implementation: a
//! fix applied to `console.log` and not to `console.error`, or to
//! `format_jsvalue` and not to `format_jsvalue_for_json` (which renders the
//! same array once it is a field of an object), is a half-fix that reads as a
//! working one.

use crate::value::JSValue;

/// Node's `util.inspect` label for a CLASS value: `[class Name]`,
/// `[class Name extends Parent]`, `[class (anonymous)]`. `None` for anything
/// that is not a registered class reference.
///
/// A class value is its function object (`js_class_value`); `class_ref_id`
/// also names a `C.prototype` reference. An int32 number is never a class,
/// whatever classes the program declares.
pub(crate) fn class_ref_inspect_label(value: f64) -> Option<String> {
    let class_id = crate::object::class_ref_id(value)?;
    Some(class_label_for_id(class_id))
}

/// `[class …]` for a resolved class id.
pub(crate) fn class_label_for_id(class_id: u32) -> String {
    // The name comes from whatever `class_name_for_id` reports — deliberately
    // NOT re-derived here. #9413 covers class `.name` leaking compiler-internal
    // spellings; when that lands, the corrected name flows straight through.
    let own = crate::object::class_name_for_id(class_id).unwrap_or_default();
    let mut out = String::with_capacity(16 + own.len());
    out.push_str("[class ");
    if own.is_empty() {
        out.push_str("(anonymous)");
    } else {
        out.push_str(&own);
    }
    if let Some(parent) = parent_class_name(class_id) {
        out.push_str(" extends ");
        out.push_str(&parent);
    }
    out.push(']');
    out
}

/// The `extends` half of the label. Node renders the heritage from the parent's
/// `.name`, so an unnamed base contributes nothing rather than a dangling
/// `extends`.
fn parent_class_name(class_id: u32) -> Option<String> {
    let parent = crate::object::get_parent_class_id(class_id)?;
    if parent == 0 || parent == class_id {
        return None;
    }
    crate::object::class_name_for_id(parent).filter(|n| !n.is_empty())
}

/// Node's rendering of an INT32-tagged NaN box. The tag is shared between small
/// integers and class references, so this is the ONE place that decides which
/// one a display ladder is looking at; every `else if v.is_int32()` arm in a
/// console/inspect ladder calls it.
pub(crate) fn int32_or_class_repr(value: f64) -> String {
    match class_ref_inspect_label(value) {
        Some(label) => label,
        None => JSValue::from_bits(value.to_bits()).as_int32().to_string(),
    }
}

/// True for the sparse-array hole sentinel (`TAG_HOLE`, #323).
///
/// A hole is not a value: `js_array_get_f64` translates it to `undefined` per
/// OrdinaryGet, and `Object.keys` / `in` inspect slots directly to tell a hole
/// from an explicit `undefined` write. Only code that reads element slots
/// *raw* — which every array formatter does — can observe one, and every such
/// reader must classify it before its ladder reaches "must be a regular
/// number", where the sentinel's bits read back as `NaN`.
#[inline]
pub(crate) fn is_array_hole(value: f64) -> bool {
    value.to_bits() == crate::value::TAG_HOLE
}

/// Node's label for a run of `count` consecutive array holes.
fn empty_items_label(count: usize) -> String {
    if count == 1 {
        "<1 empty item>".to_string()
    } else {
        format!("<{} empty items>", count)
    }
}

/// One entry of a rendered array — what Node prints between the commas. A run
/// of holes is ONE entry (`<N empty items>`), which is why the entry count and
/// the array's `length` are different numbers and why the layout decisions
/// below count entries.
pub(crate) struct ArrayEntry {
    pub(crate) text: String,
    /// Eligible for Node's right-aligned numeric column layout. False for a
    /// hole run, and false for a class reference, which shares the INT32 tag
    /// with the integers that layout is meant for.
    pub(crate) numeric: bool,
}

impl ArrayEntry {
    pub(crate) fn new(text: String, numeric: bool) -> Self {
        ArrayEntry { text, numeric }
    }
}

/// Node's entries for an array of `len` slots, fed its present elements in
/// index order: each element rendered, each *run* of absent indices (a hole,
/// or an index past a sparse Array's store with no own element) collapsed to
/// one `<N empty items>` entry.
struct EntryRuns {
    parts: Vec<ArrayEntry>,
    next: usize,
}

impl EntryRuns {
    fn new() -> Self {
        EntryRuns {
            parts: Vec::new(),
            next: 0,
        }
    }

    fn present(&mut self, index: usize, entry: ArrayEntry) {
        if index > self.next {
            self.parts
                .push(ArrayEntry::new(empty_items_label(index - self.next), false));
        }
        self.parts.push(entry);
        self.next = index + 1;
    }

    fn finish(mut self, len: usize) -> Vec<ArrayEntry> {
        if len > self.next {
            self.parts
                .push(ArrayEntry::new(empty_items_label(len - self.next), false));
        }
        self.parts
    }
}

/// The entries Node prints for the plain Array `arr`: its own elements in
/// index order, rendered by `render`, with runs of absent indices collapsed.
/// A sparse Array (`length > capacity`) keeps its far elements in its named
/// layout: they are read from there, never as slots past the store
/// (`array_for_each_own_element`), so `[]; a[3000000] = 1` is
/// `[ <3000000 empty items>, 1 ]`.
///
/// # Safety
///
/// `arr` is a live, forwarding-resolved plain Array.
pub(crate) unsafe fn array_entries<F>(
    arr: *const crate::array::ArrayHeader,
    mut render: F,
) -> Vec<ArrayEntry>
where
    F: FnMut(f64) -> ArrayEntry,
{
    let len = (*arr).length as usize;
    let mut runs = EntryRuns::new();
    crate::array::array_for_each_own_element(arr, |index, elem| {
        runs.present(index as usize, render(elem));
        true
    });
    // A render that ran program code (an inspect hook) may have shortened
    // the Array: the trailing run ends at its length now, as in node.
    runs.finish(len.min((*arr).length as usize))
}

/// [`array_entries`] over a slice of raw slots (`TAG_HOLE` = absent).
#[cfg(test)]
fn array_entries_with_holes<F>(slots: &[f64], mut render: F) -> Vec<ArrayEntry>
where
    F: FnMut(f64) -> ArrayEntry,
{
    let mut runs = EntryRuns::new();
    for (index, &elem) in slots.iter().enumerate() {
        if !is_array_hole(elem) {
            runs.present(index, render(elem));
        }
    }
    runs.finish(slots.len())
}

/// Classify a rendered element for the numeric column layout. A number or an
/// int32 qualifies — unless the int32 was a class reference, whose label is not
/// a numeral and must not be right-aligned into a column of them.
pub(crate) fn entry_is_numeric(elem: f64, rendered: &str) -> bool {
    let jv = JSValue::from_bits(elem.to_bits());
    if jv.is_int32() {
        // Both renderings come from `int32_or_class_repr`, so this is a test on
        // our own output, not a guess about arbitrary text: an integer never
        // starts with `[`.
        return !rendered.starts_with('[');
    }
    // node right-aligns a column of numbers and bigints alike.
    jv.is_number() || jv.is_bigint()
}

/// Node's `showHidden` tail for an array body. An array's own `length` is a
/// non-enumerable property, so `util.inspect(a, { showHidden: true })` — which
/// is exactly what `%o` is — prints it after the elements as `[length]: N`, on
/// EVERY array and at every depth (#9463, measured against node 26.5.1:
/// `util.format("%o", [1,2,3])` → `[ 1, 2, 3, [length]: 3 ]`, and `%o` on `[]`
/// → `[ [length]: 0 ]` where `%O` gives a bare `[]`).
pub(crate) fn hidden_length_entry(length: usize) -> ArrayEntry {
    ArrayEntry::new(format!("[length]: {}", length), false)
}

/// Lay out rendered entries as Node's `util.inspect` array body at nesting
/// `depth` (node `groupArrayElements` + `reduceToSingleString`).
///
/// The layout counts ENTRIES, not array slots: `new Array(7)` is seven slots
/// but one entry, and Node prints it as `[ <7 empty items> ]`. Entries are
/// right-aligned in columns only when every one is a number (a hole run or the
/// `showHidden` `[length]` tail is not).
pub(crate) fn array_body(parts: Vec<ArrayEntry>, depth: usize, compact: bool) -> String {
    let pad_start = parts.iter().all(|p| p.numeric);
    let texts = parts.into_iter().map(|p| p.text).collect();
    super::array_layout::reduce_array_entries(texts, depth, pad_start, compact)
}

/// Node's `util.inspect` rendering of a Promise: `Promise { <pending> }`,
/// `Promise { <value> }` once fulfilled, `Promise { <rejected> <reason> }` once
/// rejected.
///
/// Perry hard-coded the pending form, so every settled promise misreported its
/// state — `console.log(Promise.resolve(1))` said `<pending>` for a promise
/// that had already resolved. The state is a plain byte in the `Promise` cell;
/// nothing about the tag encoding was involved.
///
/// # Safety
///
/// `ptr` must be a live `GC_TYPE_PROMISE` cell.
pub(crate) unsafe fn promise_inspect<F>(ptr: *const crate::promise::Promise, mut fmt: F) -> String
where
    F: FnMut(f64) -> String,
{
    if ptr.is_null() {
        return "Promise { <pending> }".to_string();
    }
    match (*ptr).state {
        crate::promise::PromiseState::Pending => "Promise { <pending> }".to_string(),
        crate::promise::PromiseState::Fulfilled => format!("Promise {{ {} }}", fmt((*ptr).value)),
        crate::promise::PromiseState::Rejected => {
            format!("Promise {{ <rejected> {} }}", fmt((*ptr).reason))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(text: &str, numeric: bool) -> ArrayEntry {
        ArrayEntry::new(text.to_string(), numeric)
    }

    #[test]
    fn empty_items_label_singular_and_plural() {
        assert_eq!(empty_items_label(1), "<1 empty item>");
        assert_eq!(empty_items_label(2), "<2 empty items>");
        assert_eq!(empty_items_label(7), "<7 empty items>");
    }

    #[test]
    fn hole_sentinel_is_the_only_hole() {
        assert!(is_array_hole(f64::from_bits(crate::value::TAG_HOLE)));
        assert!(!is_array_hole(f64::from_bits(crate::value::TAG_UNDEFINED)));
        assert!(!is_array_hole(f64::NAN));
        assert!(!is_array_hole(0.0));
        // TDZ shares the 0x7FFC singleton namespace and must not be mistaken
        // for a hole — reading one is a ReferenceError, not an empty slot.
        assert!(!is_array_hole(f64::from_bits(crate::value::TAG_TDZ)));
    }

    #[test]
    fn hole_runs_collapse_and_non_holes_render() {
        let hole = f64::from_bits(crate::value::TAG_HOLE);
        let slots = [1.0, hole, hole, 2.0, hole];
        let parts = array_entries_with_holes(&slots, |v| ArrayEntry::new(v.to_string(), true));
        let rendered: Vec<&str> = parts.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(rendered, ["1", "<2 empty items>", "2", "<1 empty item>"]);
        assert_eq!(
            parts.iter().map(|p| p.numeric).collect::<Vec<_>>(),
            [true, false, true, false]
        );
    }

    #[test]
    fn a_whole_array_of_holes_is_one_entry() {
        let hole = f64::from_bits(crate::value::TAG_HOLE);
        let slots = [hole; 7];
        let parts = array_entries_with_holes(&slots, |_| ArrayEntry::new("?".to_string(), true));
        assert_eq!(parts.len(), 1);
        // Seven SLOTS but one ENTRY, so Node keeps it on one line.
        assert_eq!(array_body(parts, 0, true), "[ <7 empty items> ]");
    }

    /// #9463: `%o` is `util.inspect(v, { showHidden: true, depth: 4 })`, and an
    /// array's own `length` is a non-enumerable property node prints after the
    /// elements. Measured against node 26.5.1.
    #[test]
    fn the_show_hidden_tail_is_nodes_length_entry() {
        let parts = vec![entry("1", true), entry("2", true), hidden_length_entry(2)];
        assert_eq!(
            array_body(parts, 0, true),
            "[ 1, 2, [length]: 2 ]"
        );
        // An empty array still carries it: node's `%o` on `[]` is
        // `[ [length]: 0 ]` where `%O` is a bare `[]`.
        assert_eq!(
            array_body(vec![hidden_length_entry(0)], 0, true),
            "[ [length]: 0 ]"
        );
        // The tail is not numeric, so it never joins the right-aligned column
        // layout — and node's grouping stops applying once it is present, which
        // is why more than six entries still print on one line.
        let many: Vec<ArrayEntry> = (1..=8)
            .map(|n: u32| ArrayEntry::new(n.to_string(), true))
            .chain(std::iter::once(hidden_length_entry(8)))
            .collect();
        assert_eq!(
            array_body(many, 0, true),
            "[ 1, 2, 3, 4, 5, 6, 7, 8, [length]: 8 ]"
        );
    }

    #[test]
    fn single_line_layout_matches_node() {
        let parts = vec![
            entry("1", true),
            entry("<1 empty item>", false),
            entry("3", true),
        ];
        assert_eq!(array_body(parts, 0, true), "[ 1, <1 empty item>, 3 ]");
        assert_eq!(array_body(Vec::new(), 0, true), "[]");
    }

    #[test]
    fn seven_numeric_entries_still_column_wrap() {
        let parts: Vec<ArrayEntry> = (0..7).map(|i| entry(&i.to_string(), true)).collect();
        let body = array_body(parts, 0, true);
        assert!(
            body.starts_with("[\n"),
            "expected multi-line body, got {body}"
        );
    }

    #[test]
    fn an_unregistered_int32_is_not_a_class() {
        // 0 is never a class id, and the registry probe rejects the rest.
        let n = f64::from_bits(crate::value::INT32_TAG);
        assert_eq!(class_ref_inspect_label(n), None);
        assert_eq!(int32_or_class_repr(n), "0");
        let big = f64::from_bits(crate::value::INT32_TAG | 0xFFFF_FFF0);
        assert_eq!(class_ref_inspect_label(big), None);
    }

    #[test]
    fn a_plain_double_is_never_a_class() {
        assert_eq!(class_ref_inspect_label(49.0), None);
        assert_eq!(class_ref_inspect_label(f64::NAN), None);
    }

    #[test]
    fn entry_numeric_classification() {
        assert!(entry_is_numeric(49.0, "49"));
        assert!(entry_is_numeric(
            f64::from_bits(crate::value::INT32_TAG | 49),
            "49"
        ));
        assert!(!entry_is_numeric(
            f64::from_bits(crate::value::INT32_TAG | 49),
            "[class Klass]"
        ));
        assert!(!entry_is_numeric(
            f64::from_bits(crate::value::TAG_UNDEFINED),
            "undefined"
        ));
    }
}
