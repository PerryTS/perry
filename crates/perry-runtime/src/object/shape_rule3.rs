//! Rule 3 of the single-path object model: **no pointer-tagged non-object
//! cell may hold a value in the ShapeId range at payload offset `+4`.**
//!
//! The emitted read path derives its shape token from a raw 32-bit load at
//! `receiver + 4` and compares it against the site's cached ShapeId
//! (`perry-codegen/src/expr/property_get/generic_dispatch.rs`). Today that
//! compare is fenced by a GC-header load proving `obj_type == GC_TYPE_OBJECT`.
//! The point of the rule is to make that fence removable: if no other cell
//! kind can produce a `+4` word inside `[SHAPE_ID_BASE, SHAPE_ID_END)`, a
//! shape compare that succeeds has already proved the receiver is a shaped
//! ordinary object.
//!
//! Every kind's `+4` word, and why it is (or is not) safe, is enumerated by
//! [`RULE3_KINDS`] below — a table the tests walk, so a new `GC_TYPE_*`
//! cannot be added without classifying it.

#[cfg(test)]
use crate::gc;

/// What the 32-bit word at payload `+4` of a cell of this kind holds, and
/// whether a value in the ShapeId range is reachable.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Rule3Word {
    /// This IS the ShapeId word. `GC_TYPE_OBJECT` only.
    TheShapeId,
    /// Bounded below `SHAPE_ID_BASE` at every allocation site.
    BoundedBelowRange,
    /// Structurally small (enum, magic constant, pointer high half on a
    /// 47-bit address space, a length with its own much smaller cap).
    StructurallySmall,
    /// Can reach the range, but values of this kind never travel under
    /// `POINTER_TAG`, so no read path can load `+4` on one.
    NotPointerTagged,
    /// Can reach the range and IS pointer-tagged: the GC-kind fence is still
    /// required for this kind. Each variant carries the reason.
    RangeReachable(&'static str),
}

/// Every GC kind, its `+4` word, and its rule-3 verdict.
///
/// Sourced from the struct definitions, not from comments: `ArrayHeader`
/// (`array/header.rs`), `ObjectHeader` (`object/mod.rs`), `StringHeader`
/// (`string/mod.rs`), `ClosureHeader` (`closure/alloc.rs`), `Promise`
/// (`promise/mod.rs`), `BigIntHeader` (`bigint/mod.rs`), `ErrorHeader`
/// (`error.rs`), `MapHeader` (`map.rs`), `LazyArrayHeader` (`json_tape.rs`),
/// `BufferHeader` (`buffer/header.rs`), `TypedArrayHeader`
/// (`typedarray/mod.rs`), `SetHeader` (`set.rs`), the three `native_arena.rs`
/// headers, `NativeHandleHeader` (`native_handle.rs`), `DateCell` (`date.rs`),
/// `TemporalCell` (`temporal/mod.rs`), `ObjectMeta` (`object/mod.rs`),
/// `RegExpHeader` (`regex.rs`), `ProgramCell` (`regex/perex_owner.rs`).
#[cfg(test)]
pub(crate) const RULE3_KINDS: &[(u8, &str, &str, Rule3Word)] = &[
    (
        gc::GC_TYPE_ARRAY,
        "ArrayHeader",
        "capacity: u32",
        Rule3Word::RangeReachable(
            "a 2^31-element capacity needs a 16 GiB backing block; growth is \
             old*2 on u32 and is debug-asserted at array/push_pop.rs",
        ),
    ),
    (
        gc::GC_TYPE_OBJECT,
        "ObjectHeader",
        "parent_class_id / ShapeId",
        Rule3Word::TheShapeId,
    ),
    (
        gc::GC_TYPE_STRING,
        "StringHeader",
        "byte_len: u32",
        // MAX_STRING_LENGTH is 536_870_888 UTF-16 units, so byte_len tops out
        // at 1_610_612_664 — below SHAPE_ID_BASE. (A SymbolHeader shares the
        // kind and holds `registered: u32`, 0 or 1.)
        Rule3Word::StructurallySmall,
    ),
    (
        gc::GC_TYPE_CLOSURE,
        "ClosureHeader",
        "high half of func_ptr",
        Rule3Word::StructurallySmall,
    ),
    (
        gc::GC_TYPE_PROMISE,
        "Promise",
        "explicit zero pad",
        Rule3Word::BoundedBelowRange,
    ),
    (
        gc::GC_TYPE_BIGINT,
        "BigIntHeader",
        "high half of limbs[0]",
        Rule3Word::NotPointerTagged,
    ),
    (
        gc::GC_TYPE_ERROR,
        "ErrorHeader",
        "error_kind: u32",
        Rule3Word::StructurallySmall,
    ),
    (
        gc::GC_TYPE_MAP,
        "MapHeader",
        "capacity: u32",
        Rule3Word::RangeReachable("needs a 32 GiB entry buffer; debug-asserted at map.rs"),
    ),
    (
        gc::GC_TYPE_LAZY_ARRAY,
        "LazyArrayHeader",
        "magic = LAZY_ARRAY_MAGIC",
        Rule3Word::StructurallySmall,
    ),
    (
        gc::GC_TYPE_BUFFER,
        "BufferHeader",
        "capacity: u32 (bytes)",
        Rule3Word::RangeReachable(
            "every user-facing constructor caps at i32::MAX; Buffer.from(arrayLike) \
             and Buffer.concat still clamp at u32::MAX",
        ),
    ),
    (
        gc::GC_TYPE_TYPED_ARRAY,
        "TypedArrayHeader",
        "capacity: u32 (elements)",
        Rule3Word::BoundedBelowRange,
    ),
    (
        gc::GC_TYPE_SET,
        "SetHeader",
        "capacity: u32",
        Rule3Word::RangeReachable("needs a 16 GiB entry buffer; debug-asserted at set.rs"),
    ),
    (
        gc::GC_TYPE_NATIVE_ARENA_OWNER,
        "NativeArenaOwnerHeader",
        "high half of byte_length: u64",
        Rule3Word::StructurallySmall,
    ),
    (
        gc::GC_TYPE_NATIVE_TYPED_VIEW,
        "NativeTypedViewHeader",
        "capacity: u32 (elements)",
        Rule3Word::RangeReachable("debug-asserted at native_arena.rs"),
    ),
    (
        gc::GC_TYPE_NATIVE_HANDLE,
        "NativeHandleHeader",
        "high half of magic",
        Rule3Word::StructurallySmall,
    ),
    (
        gc::GC_TYPE_NATIVE_POD_VIEW,
        "NativePodViewHeader",
        "high half of owner pointer",
        Rule3Word::StructurallySmall,
    ),
    (
        gc::GC_TYPE_DATE_CELL,
        "DateCell",
        "high 32 bits of ts: f64",
        Rule3Word::RangeReachable(
            "new Date(-1) gives 0xBFF0_0000 — in range, but that is shape index \
             1_072_693_248, three orders past anything a program mints",
        ),
    ),
    (
        gc::GC_TYPE_TEMPORAL,
        "TemporalCell",
        "high half of Box<TemporalValue>",
        Rule3Word::StructurallySmall,
    ),
    (
        gc::GC_TYPE_OBJECT_META,
        "ObjectMeta",
        "high 32 of prototype NaN-box",
        Rule3Word::NotPointerTagged,
    ),
    (
        gc::GC_TYPE_REGEXP,
        "RegExpHeader",
        "high half of pattern_ptr",
        Rule3Word::StructurallySmall,
    ),
    (
        gc::GC_TYPE_REGEX_PROGRAM,
        "ProgramCell",
        "high half of word_count (< 2^30)",
        Rule3Word::NotPointerTagged,
    ),
];

/// Rule 3, checked where the word is written.
///
/// Free in release (`debug_assert!`), and in debug it names the kind whose
/// allocation would hand the emitted read path a word it cannot tell from a
/// live ShapeId. Call it beside every store of a `capacity`-shaped u32 into a
/// pointer-tagged cell's `+4`.
#[inline]
pub(crate) fn debug_assert_not_shape_id_word(kind: &'static str, word: u32) {
    debug_assert!(
        !crate::object::shapes::is_shape_id(word),
        "rule 3: {kind} stored {word:#x} at payload +4, which the emitted read \
         path cannot distinguish from a live ShapeId"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::shapes::{is_shape_id, SHAPE_ID_BASE, SHAPE_ID_END};

    /// The table must classify EVERY kind the collector knows about. Adding a
    /// `GC_TYPE_*` without a rule-3 verdict is the failure this catches — the
    /// new kind would otherwise silently inherit "nobody looked".
    #[test]
    fn rule3_table_covers_every_gc_kind() {
        for kind in 1..=gc::GC_TYPE_MAX {
            assert!(
                RULE3_KINDS.iter().any(|(k, ..)| *k == kind),
                "GC kind {kind} has no rule-3 verdict: what does it store at payload +4, \
                 and can that value land in [{SHAPE_ID_BASE:#x}, {SHAPE_ID_END:#x})?"
            );
        }
        // And nothing stale: every row names a live kind.
        for (kind, name, ..) in RULE3_KINDS {
            assert!(
                (1..=gc::GC_TYPE_MAX).contains(kind),
                "{name} names GC kind {kind}, which no longer exists"
            );
        }
    }

    /// Exactly one kind may legitimately hold a ShapeId at +4.
    #[test]
    fn rule3_only_ordinary_objects_own_the_shape_word() {
        let owners: Vec<&str> = RULE3_KINDS
            .iter()
            .filter(|(.., verdict)| *verdict == Rule3Word::TheShapeId)
            .map(|(_, name, ..)| *name)
            .collect();
        assert_eq!(owners, vec!["ObjectHeader"]);
    }

    /// The live consequence for the emitted read path: as long as any kind is
    /// `RangeReachable`, a shape compare on its own does NOT prove the
    /// receiver is an ordinary object, so the GC-kind load cannot be dropped.
    ///
    /// This test does not demand the list be empty — it pins WHICH kinds keep
    /// the fence alive, so shrinking the list is a visible change rather than
    /// a silent one.
    #[test]
    fn rule3_kinds_that_still_require_the_gc_kind_fence() {
        let mut still_reachable: Vec<&str> = RULE3_KINDS
            .iter()
            .filter(|(.., verdict)| matches!(verdict, Rule3Word::RangeReachable(_)))
            .map(|(_, name, ..)| *name)
            .collect();
        still_reachable.sort_unstable();
        assert_eq!(
            still_reachable,
            vec![
                "ArrayHeader",
                "BufferHeader",
                "DateCell",
                "MapHeader",
                "NativeTypedViewHeader",
                "SetHeader",
            ],
            "the set of kinds whose +4 word can alias a ShapeId changed — if it \
             shrank, the emitted read path's GC-kind load is closer to removable; \
             if it grew, something new can silently alias a live shape"
        );
    }

    /// Cells that really exist, checked rather than argued. Each is allocated
    /// through its production path and its `+4` word read back.
    #[test]
    fn rule3_real_cells_do_not_carry_a_shape_id_at_plus_four() {
        let _lock = crate::gc::global_side_table_test_lock();
        unsafe {
            let word_at = |addr: usize| -> u32 { *((addr + 4) as *const u32) };

            let arr = crate::array::js_array_alloc(8) as usize;
            assert!(!is_shape_id(word_at(arr)), "array capacity");

            let buf = crate::buffer::js_buffer_alloc(64, 0) as usize;
            assert!(!is_shape_id(word_at(buf)), "buffer capacity");

            let map = crate::map::js_map_alloc(8) as usize;
            assert!(!is_shape_id(word_at(map)), "map capacity");

            let set = crate::set::js_set_alloc(8) as usize;
            assert!(!is_shape_id(word_at(set)), "set capacity");

            // A Promise's +4 is struct padding: `arena_alloc_gc` hands back
            // reused, non-zeroed memory, so without an explicit zero field a
            // Promise born at a dead shaped object's address inherits its
            // LIVE ShapeId.
            let promise = crate::promise::js_promise_new() as usize;
            assert_eq!(
                word_at(promise),
                0,
                "a Promise's +4 padding must be written, not inherited from \
                 whatever cell previously occupied the address"
            );
        }
    }

    /// The typed-array length cap: `new Int8Array(2**31)` used to be admitted
    /// (the check was `u32::MAX`, while its Uint8Array / ArrayBuffer siblings
    /// stop at `i32::MAX`), and `capacity = 0x8000_0000` is the FIRST ShapeId
    /// the process ever mints.
    #[test]
    fn rule3_typed_array_length_cap_is_below_the_shape_id_range() {
        assert!(
            (i32::MAX as u32) < SHAPE_ID_BASE,
            "the typed-array element cap must stay below the ShapeId floor"
        );
        assert!(!is_shape_id(i32::MAX as u32));
    }
}
