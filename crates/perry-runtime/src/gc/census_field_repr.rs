//! Charter step 5, stage 0: the field-representation census.
//!
//! A section of the heap census (`PERRY_GC_CENSUS`, `gc/census.rs`) that
//! answers, before any shape carries a representation, what a shape-level
//! `F64` rep would find in THIS heap:
//!
//! * which (ShapeId, inline slot) pairs hold a JS Number in EVERY live object
//!   carrying the shape: the slots a key-add-time `F64` would keep;
//! * how those Numbers are boxed today (INT32 box, finite double, NaN/±Inf):
//!   an INT32 box in a would-be `F64` slot is stored as a double under step 5,
//!   which is the representation risk the design calls R2;
//! * which pairs hold a Number in some objects and a non-Number in others:
//!   the slots a lineage would have generalized (F64 → Any);
//! * the fork multiplier: distinct (ShapeId, per-object Number mask) pairs
//!   over distinct ShapeIds, the growth rep-in-identity would cost if every
//!   object kept the rep it was born with;
//! * the objects' share of the per-object layout tables (`LAYOUT_SLOT_MASKS`,
//!   `TYPED_LAYOUTS`) that step 5 deletes for objects.
//!
//! Only slots 0..32 are classified: the design gives a rep only to those
//! (the same limit 4b's region word has). Everything here runs inside the
//! census walk, uses Rust-owned buffers only, and is never compiled into a
//! path the collector takes without `PERRY_GC_CENSUS`.

use std::collections::{HashMap, HashSet};

/// The representation a design-level rep word covers.
pub(super) const REP_SLOTS: usize = 32;

/// Value classes, as bits so a (shape, slot) pair can OR every object's.
const C_INT32: u8 = 1 << 0;
const C_DOUBLE: u8 = 1 << 1;
const C_NAN_INF: u8 = 1 << 2;
const C_OTHER: u8 = 1 << 3;
const C_NUMBER: u8 = C_INT32 | C_DOUBLE | C_NAN_INF;

/// The class of one slot's bits. A Number is an INT32 box or any value outside
/// the tag band; a raw 48-bit pointer stored untagged is not a Number (the
/// same rule `census::slot_kind` applies).
#[inline]
pub(super) fn value_class(bits: u64) -> u8 {
    use crate::value::*;
    match bits & TAG_MASK {
        INT32_TAG => C_INT32,
        POINTER_TAG | STRING_TAG | SHORT_STRING_TAG | BIGINT_TAG | JS_HANDLE_TAG | TAG_MARKER => {
            C_OTHER
        }
        _ if (0x1000..=0x0000_FFFF_FFFF_FFFF).contains(&bits) => C_OTHER,
        _ if bits & 0x7FF0_0000_0000_0000 == 0x7FF0_0000_0000_0000 => C_NAN_INF,
        _ => C_DOUBLE,
    }
}

#[derive(Clone)]
struct ShapeAcc {
    objects: u64,
    /// Slots classified for this shape: `min(live_inline_slot_count, 32)`,
    /// the smallest seen (an object's capacity can clip it).
    slots: usize,
    classes: [u8; REP_SLOTS],
    int32_boxes: [u32; REP_SLOTS],
    doubles: [u32; REP_SLOTS],
    nan_inf: [u32; REP_SLOTS],
}

#[derive(Default)]
pub(super) struct FieldReprCensus {
    shapes: HashMap<u32, ShapeAcc>,
    /// (ShapeId, per-object Number mask over slots 0..32).
    forks: HashSet<(u32, u32)>,
    mask_entries: u64,
    mask_bytes: u64,
    typed_entries: u64,
    typed_bytes: u64,
}

impl FieldReprCensus {
    /// One live shaped object: its stamp, its first `live` inline slots, and
    /// its user address (the per-object tables' key).
    ///
    /// # Safety
    /// `slots` addresses at least `live` readable words.
    pub(super) unsafe fn note_object(
        &mut self,
        stamp: u32,
        slots: *const u64,
        live: usize,
        user: usize,
    ) {
        let n = live.min(REP_SLOTS);
        let acc = self.shapes.entry(stamp).or_insert_with(|| ShapeAcc {
            objects: 0,
            slots: n,
            classes: [0; REP_SLOTS],
            int32_boxes: [0; REP_SLOTS],
            doubles: [0; REP_SLOTS],
            nan_inf: [0; REP_SLOTS],
        });
        acc.objects += 1;
        acc.slots = acc.slots.min(n);
        let mut number_mask = 0u32;
        for i in 0..n {
            let class = value_class(*slots.add(i));
            acc.classes[i] |= class;
            match class {
                C_INT32 => acc.int32_boxes[i] += 1,
                C_DOUBLE => acc.doubles[i] += 1,
                C_NAN_INF => acc.nan_inf[i] += 1,
                _ => {}
            }
            if class & C_NUMBER != 0 {
                number_mask |= 1 << i;
            }
        }
        self.forks.insert((stamp, number_mask));
        self.note_tables(user);
    }

    fn note_tables(&mut self, user: usize) {
        if let Ok(masks) = super::hot_tls::hot_layout_slot_masks().try_borrow() {
            if let Some(mask) = masks.get(&user) {
                self.mask_entries += 1;
                self.mask_bytes += entry_bytes::<super::layout::LayoutSlotMask>()
                    + match mask {
                        super::layout::LayoutSlotMask::Heap(words) => words.capacity() as u64 * 8,
                        _ => 0,
                    };
            }
        }
        if let Ok(typed) = super::hot_tls::hot_typed_layouts().try_borrow() {
            if typed.contains_key(&user) {
                self.typed_entries += 1;
                self.typed_bytes += entry_bytes::<super::layout::TypedLayoutDescriptor>();
            }
        }
    }

    /// The census section. Counts are over live objects of this collection.
    pub(super) fn summary(&self) -> serde_json::Value {
        let mut shapes_with_f64 = 0u64;
        let mut objects_with_f64 = 0u64;
        let mut f64_slots = 0u64;
        let mut f64_object_slots = 0u64;
        let mut f64_int32_boxes = 0u64;
        let mut f64_doubles = 0u64;
        let mut f64_nan_inf = 0u64;
        let mut mixed_slots = 0u64;
        let mut any_slots = 0u64;
        let mut live_objects = 0u64;
        for acc in self.shapes.values() {
            live_objects += acc.objects;
            let mut has_f64 = false;
            for i in 0..acc.slots {
                let class = acc.classes[i];
                if class & C_NUMBER != 0 && class & C_OTHER == 0 {
                    has_f64 = true;
                    f64_slots += 1;
                    f64_object_slots += acc.objects;
                    f64_int32_boxes += u64::from(acc.int32_boxes[i]);
                    f64_doubles += u64::from(acc.doubles[i]);
                    f64_nan_inf += u64::from(acc.nan_inf[i]);
                } else {
                    any_slots += 1;
                    if class & C_NUMBER != 0 {
                        mixed_slots += 1;
                    }
                }
            }
            if has_f64 {
                shapes_with_f64 += 1;
                objects_with_f64 += acc.objects;
            }
        }
        let live_shapes = self.shapes.len() as u64;
        let fork_multiplier = if live_shapes == 0 {
            1.0
        } else {
            self.forks.len() as f64 / live_shapes as f64
        };
        serde_json::json!({
            "live_shapes": live_shapes,
            "live_objects": live_objects,
            "shapes_with_f64": shapes_with_f64,
            "objects_with_f64": objects_with_f64,
            "f64_slots": f64_slots,
            "any_slots": any_slots,
            "mixed_slots": mixed_slots,
            "f64_object_slots": f64_object_slots,
            "f64_int32_boxes": f64_int32_boxes,
            "f64_doubles": f64_doubles,
            "f64_nan_inf": f64_nan_inf,
            "shape_mask_pairs": self.forks.len(),
            "fork_multiplier": fork_multiplier,
            "object_slot_mask_entries": self.mask_entries,
            "object_slot_mask_bytes": self.mask_bytes,
            "object_typed_layout_entries": self.typed_entries,
            "object_typed_layout_bytes": self.typed_bytes,
        })
    }
}

/// The resident cost of one hash-map entry keyed by an address: the key, the
/// value, and hashbrown's control byte.
fn entry_bytes<V>() -> u64 {
    (std::mem::size_of::<usize>() + std::mem::size_of::<V>() + 1) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::*;

    fn int32(v: i32) -> u64 {
        INT32_TAG | u64::from(v as u32)
    }
    const STRING: u64 = STRING_TAG | 0x1234_5678;

    fn census(objects: &[(u32, &[u64])]) -> serde_json::Value {
        let mut c = FieldReprCensus::default();
        for (stamp, slots) in objects {
            // SAFETY: `slots` is a live slice of `len` words.
            unsafe { c.note_object(*stamp, slots.as_ptr(), slots.len(), 0) };
        }
        c.summary()
    }

    #[test]
    fn value_classes_split_number_from_everything_else() {
        assert_eq!(value_class(int32(7)), C_INT32);
        assert_eq!(value_class(1.5f64.to_bits()), C_DOUBLE);
        assert_eq!(value_class(0.0f64.to_bits()), C_DOUBLE);
        assert_eq!(value_class(f64::NAN.to_bits()), C_NAN_INF);
        assert_eq!(value_class(f64::INFINITY.to_bits()), C_NAN_INF);
        assert_eq!(value_class(f64::NEG_INFINITY.to_bits()), C_NAN_INF);
        assert_eq!(value_class(STRING), C_OTHER);
        assert_eq!(value_class(TAG_UNDEFINED), C_OTHER);
        assert_eq!(value_class(TAG_NULL), C_OTHER);
        assert_eq!(value_class(POINTER_TAG | 0x10_0000), C_OTHER);
        assert_eq!(value_class(0x7f00_1234_5678), C_OTHER, "untagged pointer");
    }

    /// The falsifier: a Number slot of shape 1 is F64, a slot holding a string
    /// in one object of three is a generalized (mixed) slot, and every other
    /// fact counts objects, not shapes.
    #[test]
    fn a_non_number_in_one_object_makes_the_slot_mixed() {
        let a = [int32(1), STRING, 2.5f64.to_bits()];
        let b = [int32(2), STRING, 3.5f64.to_bits()];
        let sabotaged = [int32(3), STRING, STRING];
        let s = census(&[(1, &a), (1, &b), (1, &sabotaged), (2, &[STRING])]);
        assert_eq!(s["live_shapes"], 2);
        assert_eq!(s["live_objects"], 4);
        assert_eq!(s["shapes_with_f64"], 1);
        assert_eq!(s["objects_with_f64"], 3);
        assert_eq!(s["f64_slots"], 1, "only slot 0 of shape 1");
        assert_eq!(s["f64_object_slots"], 3);
        assert_eq!(s["f64_int32_boxes"], 3);
        assert_eq!(s["mixed_slots"], 1, "slot 2 of shape 1 generalized");
        assert_eq!(s["any_slots"], 3);
        // shape 1: masks 0b101 and 0b001; shape 2: 0.
        assert_eq!(s["shape_mask_pairs"], 3);
    }

    #[test]
    fn only_the_first_32_slots_are_classified() {
        let slots: Vec<u64> = (0..40).map(int32).collect();
        let s = census(&[(9, &slots)]);
        assert_eq!(s["f64_slots"], REP_SLOTS as u64);
        assert_eq!(s["f64_nan_inf"], 0);
    }
}
