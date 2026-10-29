//! Positive own-slot lookup whose authority is the receiver's live shape.
use super::{ShapeObjectKind, ShapeSlab, PROTO_ID_DEFAULT, PROTO_ID_NULL, PROTO_ID_PER_OBJECT};
use crate::array::ArrayHeader;
use crate::codegen_abi::LayoutWord;
use crate::object::ObjectHeader;

/// Owner facts borrowed for one callback-free read. No allocation or shape
/// mutation may occur between obtaining these facts and using them.
#[derive(Clone, Copy)]
pub(crate) struct OwnDataShape {
    pub(crate) keys: u64,
    pub(crate) logical_key_count: u32,
    pub(crate) live_inline_slot_count: u32,
    plain_bound: u32,
    pub(crate) summary: u8,
    pub(crate) proto_id: u64,
}

/// A live ordinary layout, a proven other layout (`Some(None)`), or no proof
/// (`None`). Preserve the first receiver's facts even when a plain slot cannot
/// be served, so the generic data walk need not classify it a second time.
///
/// Rule 3 permits asking the shape directory with the receiver's +4 word
/// before classifying the cell: only shaped objects can carry a live ShapeId.
#[inline]
pub(crate) unsafe fn own_data_shape(
    dir: *const u8,
    shape_id: LayoutWord,
) -> Option<Option<OwnDataShape>> {
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
        || record.summary()
            & (crate::object::key_attrs::SUMMARY_PRIVATE | crate::object::key_attrs::SUMMARY_HOLES)
            != 0
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
        proto_id: record.proto_id,
    }))
}

/// May a definite miss from the data walk (`native_get::try_data_lookup_key`
/// answering `Some(None)`) stand for the whole `[[Get]]` of `key` on `object`?
///
/// The walk proves the key is listed by no link of the chain. The generic read
/// also answers keys no link lists, for receivers whose reads are not decided
/// by their shape alone: class instances and their getters, brands, exotic and
/// native-backed cells, `arguments`, `process.env`, per-object prototypes. This
/// admits only an ordinary plain object: either one the runtime birthed class-
/// less and marked (`OBJ_FLAG_PLAIN_ORDINARY`, no class id), or an instance of
/// an object literal whose live shape and absent CLASS holder prove record
/// semantics (the same receiver proof the thenable probe uses). Its live
/// shape must be the plain `Ordinary` kind and its
/// [[Prototype]] the realm default or null by identity, never a recorded link.
/// Anything else keeps the generic read.
///
/// # Safety
/// `object` is a pointer the data walk accepted as a shaped `ObjectHeader`.
#[inline]
pub(crate) unsafe fn plain_receiver_miss_is_final(object: *const ObjectHeader, key: &[u8]) -> bool {
    use crate::gc::{
        GC_FLAG_ARENA, GC_FLAG_FORWARDED, GC_OBJECT_CALLABLE, GC_TYPE_OBJECT, OBJ_FLAG_ARGUMENTS,
        OBJ_FLAG_HAS_DESCRIPTORS, OBJ_FLAG_PLAIN_ORDINARY, OBJ_FLAG_TYPED_ARRAY_PROTO,
    };
    // `@@name` keys carry the well-known-symbol members class surfaces answer.
    if key.starts_with(b"@@") {
        return false;
    }
    let addr = object as usize;
    if !crate::value::addr_class::is_above_handle_band(addr)
        || !addr.is_multiple_of(std::mem::align_of::<ObjectHeader>())
    {
        return false;
    }
    // A literal `{ ... }` is an instance of a synthetic anon-shape class, so its
    // class id is nonzero and it carries no birth mark; the class identity is
    // the proof there. A class-less receiver needs the birth mark instead.
    let class_id = (*object).class_id;
    let classless = class_id == 0;
    let header = &*crate::gc::header_from_trusted_user_ptr(object.cast());
    const BLOCKING: u16 = OBJ_FLAG_TYPED_ARRAY_PROTO
        | OBJ_FLAG_ARGUMENTS
        | GC_OBJECT_CALLABLE
        | OBJ_FLAG_HAS_DESCRIPTORS;
    if !crate::gc::object_has_meta_word(header.obj_type)
        || header.gc_flags & (GC_FLAG_ARENA | GC_FLAG_FORWARDED) != GC_FLAG_ARENA
        || header._reserved & BLOCKING != 0
        || (classless && header._reserved & OBJ_FLAG_PLAIN_ORDINARY == 0)
    {
        return false;
    }
    let meta = (*object).meta;
    if !meta.is_null()
        && (*meta).flags
            & (crate::object::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER
                | crate::object::OBJECT_META_FLAG_NATIVE_ALIAS
                | crate::object::meta_flags::OBJECT_META_FLAG_SEALED_INTRINSIC)
            != 0
    {
        return false;
    }
    let shape_id = crate::object::layout_desc::layout_id_of_word((*object).parent_class_id);
    let Some(record) = ShapeSlab::ordinary_record_in(
        super::ordinary_dir_addr(),
        crate::codegen_abi::LayoutWord(shape_id),
    ) else {
        return false;
    };
    if !classless && !super::shape_is_plain_record(class_id, record.proto_id, record.object_kind())
    {
        return false;
    }
    if record.object_kind() != ShapeObjectKind::Ordinary
        || record.weak_collection_brand().is_some()
        || !record.brands().is_empty()
    {
        return false;
    }
    let recorded = super::object_prototype_word(object);
    match record.proto_id {
        PROTO_ID_DEFAULT => recorded == 0,
        PROTO_ID_NULL => recorded == 0 || recorded == crate::value::TAG_NULL,
        _ => false,
    }
}

impl OwnDataShape {
    /// `key` is the text of `key_bits` (zero when only bytes are available).
    #[inline]
    pub(crate) unsafe fn plain_lookup(
        self,
        key_bits: u64,
        key: &[u8],
    ) -> Option<Option<(u32, u32)>> {
        let bound = self.plain_bound;
        if bound == 0 || self.logical_key_count >= crate::object::KEYS_INDEX_THRESHOLD {
            return None;
        }
        // POSBOUND proves ordinary data layout, a live canonical key list, no
        // accessors, no holes and no semantic descriptor generation.
        let keys = self.keys as usize as *const ArrayHeader;
        let count = self.logical_key_count;
        crate::object::key_attrs::debug_assert_positional(keys, bound);
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
                    .then_some(Some((slot as u32, self.live_inline_slot_count)));
            }
        }
        Some(None)
    }

    #[cfg(test)]
    pub(crate) unsafe fn plain_slot(self, key_bits: u64, key: &[u8]) -> Option<(u32, u32)> {
        self.plain_lookup(key_bits, key).flatten()
    }
}
