//! Positive own-slot lookup whose authority is the receiver's live shape.
use super::{ShapeSlab, PROTO_ID_PER_OBJECT};
use crate::array::ArrayHeader;

/// Owner facts borrowed for one callback-free read. No allocation or shape
/// mutation may occur between obtaining these facts and using them.
#[derive(Clone, Copy)]
pub(crate) struct OwnDataShape {
    pub(crate) keys: u64,
    pub(crate) logical_key_count: u32,
    pub(crate) live_inline_slot_count: u32,
    plain_bound: u32,
    pub(crate) summary: u8,
}

/// A live ordinary layout, a proven other layout (`Some(None)`), or no proof
/// (`None`). Preserve the first receiver's facts even when a plain slot cannot
/// be served, so the generic data walk need not classify it a second time.
///
/// Rule 3 permits asking the shape directory with the receiver's +4 word
/// before classifying the cell: only shaped objects can carry a live ShapeId.
#[inline]
pub(crate) unsafe fn own_data_shape(dir: *const u8, shape_id: u32) -> Option<Option<OwnDataShape>> {
    let record = ShapeSlab::ordinary_record_in(dir, shape_id)?;
    let bound = record.position_bound_raw();
    if bound == 0 {
        // The generic data lane also declines nonordinary layouts. Reuse
        // that owner fact before repeating its key hash and cell walk. An
        // absent slab entry is EMPTY, not a live classification proof.
        if !record.present() {
            return None;
        }
        if !record.object_kind().is_ordinary_layout() {
            return Some(None);
        }
    }
    let plain_bound = if record.proto_id == PROTO_ID_PER_OBJECT
        || record.summary() & crate::object::key_attrs::SUMMARY_PRIVATE != 0
    {
        0
    } else {
        bound
    };
    Some(Some(OwnDataShape {
        keys: record.keys,
        logical_key_count: record.logical_key_count,
        live_inline_slot_count: record.live_inline_slot_count,
        plain_bound,
        summary: record.summary(),
    }))
}

impl OwnDataShape {
    /// `key` is the text of `key_bits` (zero when only bytes are available).
    #[inline]
    pub(crate) unsafe fn plain_slot(self, key_bits: u64, key: &[u8]) -> Option<(u32, u32)> {
        let bound = self.plain_bound;
        if bound == 0 || self.logical_key_count >= crate::object::KEYS_INDEX_THRESHOLD {
            return None;
        }
        // POSBOUND proves ordinary data layout, a live canonical key list, no
        // accessors, no holes and no semantic descriptor generation.
        let keys = self.keys as usize as *const ArrayHeader;
        let count = self.logical_key_count;
        let n = (count as usize).min((*keys).length.min((*keys).capacity) as usize);
        let words = crate::array::array_elements_ptr(keys) as *const u64;
        // Last matching key wins, including a later noncanonical spelling of an
        // earlier atom. Do not run an identity pass ahead of the byte comparisons.
        for slot in (0..n).rev() {
            let word = *words.add(slot);
            if (key_bits != 0 && word == key_bits)
                || crate::string::js_string_key_matches_bytes(crate::JSValue::from_bits(word), key)
            {
                return ((slot as u32) < bound)
                    .then_some((slot as u32, self.live_inline_slot_count));
            }
        }
        None
    }
}
