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
//! Every kind's `+4` word, and why it is safe, is enumerated by
//! [`RULE3_KINDS`] below — a table the tests walk, so a new `GC_TYPE_*`
//! cannot be added without classifying it.
//!
//! Two mechanisms close the rule, and which one a kind uses is a property of
//! what the word *is*:
//!
//! * A **count** (array/map/set/buffer/typed-array/native-view capacity) is
//!   bounded in RELEASE at its allocation and growth funnels by
//!   [`checked_plus_four_word`] / `array_capacity_or_throw`, which reject a
//!   request at or above [`MAX_PLUS_FOUR_WORD`] with the kind's own
//!   `RangeError` *before* any memory is reserved. A debug assertion beside
//!   each `+4` store is the backstop for a funnel someone adds later.
//! * A word that is **arbitrary data** is moved. `DateCell` held an `f64`
//!   time value at `+0`, and `new Date(-1)` puts `0xBFF0_0000` at `+4` —
//!   inside the ShapeId range with no bound to appeal to. The cell is
//!   relaid out so `+0..8` is its `ObjectMeta` edge and the timestamp starts
//!   at `+8`; a heap pointer's high half is bounded by the collector's own
//!   address ceiling ([`MAX_HEAP_ADDR_EXCLUSIVE`]), which is the argument
//!   eight other kinds in the table already rest on.

#[cfg(test)]
use crate::gc;

use crate::object::shapes::SHAPE_ID_BASE;

/// The largest value any cell that is **not** a `GC_TYPE_OBJECT` may carry at
/// payload `+4`: one below the ShapeId floor, i.e. `i32::MAX`.
///
/// Every count that lands in that word is checked against this at its
/// allocation funnel. The bound is not a new policy invented here — the
/// `Buffer`, `ArrayBuffer` and typed-array constructors already stop at
/// `i32::MAX` (`buffer/validate.rs`, `buffer/from.rs`,
/// `typedarray::typed_array_length_or_throw`) because the backing block is
/// sized by an `i32`. Rule 3 extends the same ceiling to the paths that had
/// been clamping at `u32::MAX` instead.
pub(crate) const MAX_PLUS_FOUR_WORD: u32 = SHAPE_ID_BASE - 1;

const _: () = assert!(MAX_PLUS_FOUR_WORD == i32::MAX as u32);

/// Exclusive ceiling on any address the collector can hand out, taken from
/// `value::addr_class` / `perry-codegen`'s `heap_addr_upper_bound_exclusive`
/// (48-bit on Linux AArch64, 47-bit everywhere else — this is the looser of
/// the two).
///
/// The rows classified [`Rule3Word::StructurallySmall`] "high half of a
/// pointer" rest on this: the top 32 bits of any address below the ceiling
/// are at most `0xFFFF`, far below the ShapeId floor.
/// A pointer that violated it would already have broken every `GcHeader`
/// probe in the runtime, so this is an invariant rule 3 inherits rather than
/// one it introduces.
pub(crate) const MAX_HEAP_ADDR_EXCLUSIVE: u64 = 0x1_0000_0000_0000;

const _: () = assert!(((MAX_HEAP_ADDR_EXCLUSIVE - 1) >> 32) < SHAPE_ID_BASE as u64);

/// What the 32-bit word at payload `+4` of a cell of this kind holds, and
/// whether a value in the ShapeId range is reachable.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Rule3Word {
    /// This IS the ShapeId word. `GC_TYPE_OBJECT` only.
    TheShapeId,
    /// Bounded below `SHAPE_ID_BASE` in RELEASE at every allocation and
    /// growth funnel that writes it.
    BoundedBelowRange,
    /// Structurally small (enum, magic constant, pointer high half under
    /// [`MAX_HEAP_ADDR_EXCLUSIVE`], a length with its own much smaller cap).
    StructurallySmall,
    /// Can reach the range, but values of this kind never travel under
    /// `POINTER_TAG`, so no read path can load `+4` on one.
    NotPointerTagged,
    /// Can reach the range and IS pointer-tagged: the GC-kind fence would
    /// still be required for this kind.
    ///
    /// Deliberately never constructed since the rule was closed. It is kept
    /// so that a kind which genuinely cannot be bounded is classified
    /// honestly instead of being forced into a verdict that does not hold —
    /// and `rule3_no_kind_still_requires_the_gc_kind_fence` fails the moment
    /// a row uses it again, which is exactly the signal lane 4 needs.
    #[allow(dead_code)]
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
        "capacity: u32 — `array_capacity_or_throw` at every alloc funnel and \
         at `js_array_grow`; `RangeError: Invalid array length`",
        Rule3Word::BoundedBelowRange,
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
        "capacity: u32 — checked at `js_map_alloc` and at the doubling in \
         `map::ensure_capacity`; `RangeError: Map maximum size exceeded`",
        Rule3Word::BoundedBelowRange,
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
        "capacity: u32 (bytes) — `buffer_alloc` and `alloc_shared_sab` throw \
         `RangeError: Array buffer allocation failed`; the foreign-span \
         wrappers clamp, as `instance_memory_span` already did",
        Rule3Word::BoundedBelowRange,
    ),
    (
        gc::GC_TYPE_TYPED_ARRAY,
        "TypedArrayHeader",
        "capacity: u32 (elements) — `typed_array_length_or_throw` plus the \
         check in `typed_array_alloc`",
        Rule3Word::BoundedBelowRange,
    ),
    (
        gc::GC_TYPE_SET,
        "SetHeader",
        "capacity: u32 — checked at `js_set_alloc` and at the doubling in \
         `set::ensure_capacity`; `RangeError: Set maximum size exceeded`",
        Rule3Word::BoundedBelowRange,
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
        "capacity: u32 (elements) — `js_native_arena_view` rejects a length \
         above the cap before it resolves the owner",
        Rule3Word::BoundedBelowRange,
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
        "high half of the meta pointer — the f64 timestamp moved to +8, \
         because `new Date(-1)` put 0xBFF0_0000 here and no bound applies to \
         a time value",
        Rule3Word::StructurallySmall,
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
/// live ShapeId. This is the BACKSTOP: the release-mode rejection lives at
/// the allocation funnel ([`checked_plus_four_word`]), so this fires only for
/// a store site that reached `+4` without passing one.
#[inline]
pub(crate) fn debug_assert_not_shape_id_word(kind: &'static str, word: u32) {
    debug_assert!(
        !crate::object::shapes::is_shape_id(word),
        "rule 3: {kind} stored {word:#x} at payload +4, which the emitted read \
         path cannot distinguish from a live ShapeId"
    );
}

/// Rule 3, enforced in RELEASE at an allocation or growth funnel.
///
/// Returns `word` when it is representable at `+4`, and otherwise throws
/// `message` as a `RangeError` — before the caller reserves any memory, so a
/// rejected request costs nothing and cannot be mistaken for an out-of-memory
/// failure. `message` is the kind's own existing over-range text, so the
/// observable behaviour at the boundary matches what the same kind already
/// does one byte further out.
#[inline]
#[must_use]
pub(crate) fn checked_plus_four_word(word: u32, message: &'static [u8]) -> u32 {
    if word > MAX_PLUS_FOUR_WORD {
        throw_plus_four_range_error(message)
    }
    word
}

/// Clamp a foreign span's byte count to what `+4` can represent.
///
/// Used only by the wrappers over memory Perry does not own
/// (`buffer_alloc_foreign`, `rebind_foreign_buffer`), which are reached from
/// `extern "C"` Node-API entry points where a JS throw has nowhere to land.
/// Every such entry point rejects an over-range length itself, so in practice
/// this never clamps — and clamping is the policy `instance_memory_span`
/// already applied to a wasm memory larger than an `i32` byte count: the
/// excess stays invisible to JS rather than wrapping the header.
#[inline]
#[must_use]
pub(crate) fn clamp_plus_four_word(kind: &'static str, word: u32) -> u32 {
    debug_assert!(
        word <= MAX_PLUS_FOUR_WORD,
        "rule 3: {kind} was handed {word:#x} for payload +4, so some entry \
         point let an over-range length through and this clamp is silently \
         shortening a span"
    );
    word.min(MAX_PLUS_FOUR_WORD)
}

#[cold]
#[inline(never)]
fn throw_plus_four_range_error(message: &'static [u8]) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_rangeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::shapes::{is_shape_id, SHAPE_ID_END};

    /// The message of a thrown `RangeError`, read straight out of the
    /// `ErrorHeader` (the pattern `global_this_webassembly`'s tests use).
    fn thrown_message(exception: f64) -> String {
        let ptr = crate::value::js_nanbox_get_pointer(exception);
        assert!(ptr != 0, "a thrown value must be an error pointer");
        unsafe {
            let err = ptr as *const crate::error::ErrorHeader;
            let message = (*err).message;
            if message.is_null() {
                return String::new();
            }
            let len = (*message).byte_len as usize;
            let data =
                (message as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
            String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
        }
    }

    /// Run `f`, expecting it to throw, and return the thrown message.
    ///
    /// Every caller hands this an allocation request AT the ShapeId floor.
    /// If the funnel's rule-3 check were removed the closure would instead
    /// try to reserve 2-32 GiB, so `expect` here is not the only thing
    /// holding the test up — but the message assertion is what distinguishes
    /// "rejected by rule 3" from "the allocator gave up", which is the
    /// failure mode that would otherwise let this test pass for free.
    fn expect_throw(f: impl FnOnce()) -> String {
        let outcome = crate::exception::catch_js_throw(f);
        match outcome {
            Ok(()) => panic!("expected a RangeError, but the call returned"),
            Err(exception) => thrown_message(exception),
        }
    }

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

    /// The live consequence for the emitted read path, and the whole point of
    /// this module: **no kind keeps the GC-kind fence alive any more.**
    ///
    /// While this list is empty, for any pointer-tagged value that passes the
    /// tag test the u32 at payload +4 equals a live object ShapeId only if
    /// the cell is a `GC_TYPE_OBJECT` carrying that shape — so a successful
    /// shape compare on its own proves the receiver is a shaped ordinary
    /// object, and `generic_dispatch` does not need to load the GC header to
    /// find that out.
    #[test]
    fn rule3_no_kind_still_requires_the_gc_kind_fence() {
        let still_reachable: Vec<&str> = RULE3_KINDS
            .iter()
            .filter(|(.., verdict)| matches!(verdict, Rule3Word::RangeReachable(_)))
            .map(|(_, name, ..)| *name)
            .collect();
        assert_eq!(
            still_reachable,
            Vec::<&str>::new(),
            "a kind's +4 word can alias a live ShapeId again — the emitted \
             read path's GC-kind load is NOT removable while this list is \
             non-empty, and any lane that already dropped it is now reading \
             a non-object's word as a shape"
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

            let ta =
                crate::typedarray::typed_array_alloc(crate::typedarray::KIND_INT8, 32) as usize;
            assert!(!is_shape_id(word_at(ta)), "typed array capacity");

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
        assert_eq!(MAX_PLUS_FOUR_WORD, i32::MAX as u32);
        assert!(!is_shape_id(MAX_PLUS_FOUR_WORD));
        assert!(is_shape_id(MAX_PLUS_FOUR_WORD + 1));
    }

    /// `SHAPE_ID_BASE` is the first capacity every count-carrying kind must
    /// refuse — it is not merely inside the range, it is the very first id
    /// the process mints, so a cell admitted with it aliases shape #1.
    ///
    /// These run the real funnels. Without the rule-3 check each one would
    /// instead ask the allocator for 2-32 GiB, and the message assertion
    /// separates "refused by rule 3" from "the allocator refused".
    #[test]
    fn rule3_array_allocation_at_the_shape_id_floor_is_refused() {
        let _lock = crate::gc::global_side_table_test_lock();
        assert_eq!(
            expect_throw(|| {
                crate::array::js_array_alloc(SHAPE_ID_BASE);
            }),
            "Invalid array length"
        );
        assert_eq!(
            expect_throw(|| {
                crate::array::js_array_alloc_with_length(SHAPE_ID_BASE);
            }),
            "Invalid array length"
        );
        assert_eq!(
            expect_throw(|| {
                crate::array::js_array_alloc_literal(SHAPE_ID_BASE);
            }),
            "Invalid array length"
        );
    }

    /// Growth has its own funnel, and its own arithmetic: `new_capacity` is
    /// `max(old * 2, min_capacity)` on `u32`, so before the bound the doubling
    /// could also wrap. A tiny array plus an over-range `min_capacity` reaches
    /// it without allocating anything.
    #[test]
    fn rule3_array_growth_to_the_shape_id_floor_is_refused() {
        let _lock = crate::gc::global_side_table_test_lock();
        let arr = crate::array::js_array_alloc(4);
        let scope = crate::gc::RuntimeHandleScope::new();
        let handle = scope.root_raw_mut_ptr(arr);
        assert_eq!(
            expect_throw(|| {
                crate::array::js_array_grow(
                    handle.get_raw_mut_ptr::<crate::array::ArrayHeader>(),
                    SHAPE_ID_BASE,
                );
            }),
            "Invalid array length"
        );
    }

    #[test]
    fn rule3_map_and_set_allocation_at_the_shape_id_floor_are_refused() {
        let _lock = crate::gc::global_side_table_test_lock();
        assert_eq!(
            expect_throw(|| {
                crate::map::js_map_alloc(SHAPE_ID_BASE);
            }),
            "Map maximum size exceeded"
        );
        assert_eq!(
            expect_throw(|| {
                crate::set::js_set_alloc(SHAPE_ID_BASE);
            }),
            "Set maximum size exceeded"
        );
    }

    #[test]
    fn rule3_buffer_allocation_at_the_shape_id_floor_is_refused() {
        let _lock = crate::gc::global_side_table_test_lock();
        assert_eq!(
            expect_throw(|| {
                crate::buffer::buffer_alloc(SHAPE_ID_BASE);
            }),
            "Array buffer allocation failed"
        );
        assert_eq!(
            expect_throw(|| {
                crate::shared_sab::alloc_shared_sab(SHAPE_ID_BASE);
            }),
            "Array buffer allocation failed"
        );
    }

    #[test]
    fn rule3_typed_array_allocation_at_the_shape_id_floor_is_refused() {
        let _lock = crate::gc::global_side_table_test_lock();
        assert_eq!(
            expect_throw(|| {
                crate::typedarray::typed_array_alloc(crate::typedarray::KIND_INT8, SHAPE_ID_BASE);
            }),
            "Array buffer allocation failed"
        );
    }

    /// The native-arena view's cap is checked BEFORE the owner is resolved,
    /// so a 16-byte arena is enough to reach it — and the distinct message
    /// proves the request was refused by rule 3 rather than by the ordinary
    /// "view is out of bounds" test that a 2 GiB view over a 16-byte arena
    /// would also fail.
    #[test]
    fn rule3_native_arena_view_length_at_the_shape_id_floor_is_refused() {
        let _lock = crate::gc::global_side_table_test_lock();
        let owner = crate::native_arena::js_native_arena_alloc(16);
        assert_eq!(
            expect_throw(|| {
                crate::native_arena::js_native_arena_view(
                    owner as u64,
                    crate::typedarray::KIND_INT8 as i32,
                    0,
                    SHAPE_ID_BASE as i64,
                );
            }),
            "NativeArena view length exceeds Perry's maximum"
        );
        crate::native_arena::js_native_arena_dispose(owner as u64);
    }

    /// `DateCell`'s relayout, with the value that motivated it.
    ///
    /// `-1.0` is `0xBFF0_0000_0000_0000`: with the timestamp at `+0` its high
    /// word landed at `+4` as `0xBFF0_0000`, a live-looking ShapeId. The
    /// first assertion pins that the witness is real, so this test cannot
    /// quietly stop testing anything; the rest check every date the JS spec
    /// admits and that the value still round-trips through the new layout.
    #[test]
    fn rule3_date_cell_never_carries_a_shape_id_at_plus_four() {
        let _lock = crate::gc::global_side_table_test_lock();
        assert!(
            is_shape_id((f64::to_bits(-1.0) >> 32) as u32),
            "the witness must still be a witness: the high word of -1.0 is \
             inside the ShapeId range, which is why the timestamp cannot live \
             at payload +0"
        );

        // Both ends of the JS time-value range, the motivating -1, the
        // Invalid Date, and both zeroes.
        let timestamps = [
            -1.0,
            0.0,
            -0.0,
            1.0,
            -8.64e15,
            8.64e15,
            f64::NAN,
            -1234.5e10,
        ];
        for ts in timestamps {
            let value = crate::date::alloc_date_cell(ts);
            let addr = crate::value::js_nanbox_get_pointer(value) as usize;
            assert!(addr != 0, "a Date is a NaN-boxed cell pointer");
            let word = unsafe { *((addr + 4) as *const u32) };
            assert!(
                !is_shape_id(word),
                "DateCell for ts {ts} put {word:#x} at payload +4"
            );
            let read_back = crate::date::date_cell_timestamp(value);
            if ts.is_nan() {
                assert!(read_back.is_nan(), "Invalid Date must stay invalid");
            } else {
                assert_eq!(
                    read_back.to_bits(),
                    ts.to_bits(),
                    "the relaid-out cell must return the same time value"
                );
            }
        }
    }

    /// The pointer-high-half rows are only sound while the collector's
    /// address ceiling stays where `value::addr_class` puts it.
    #[test]
    fn rule3_heap_pointer_high_half_cannot_reach_the_shape_id_range() {
        assert!(((MAX_HEAP_ADDR_EXCLUSIVE - 1) >> 32) < SHAPE_ID_BASE as u64);
        let arr = crate::array::js_array_alloc(1) as usize;
        assert!(
            (arr as u64) < MAX_HEAP_ADDR_EXCLUSIVE,
            "a real GC address must sit under the ceiling the \
             StructurallySmall pointer rows appeal to"
        );
        assert!(!is_shape_id((arr >> 32) as u32));
    }
}
