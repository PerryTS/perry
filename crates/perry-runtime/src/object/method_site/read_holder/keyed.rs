//! The shapes' walk for a computed key: the receiver's chain, proved hop by
//! hop from shape facts, without allocating or running anything.
use super::*;
use crate::codegen_abi::LayoutWord;

#[derive(Clone, Copy)]
pub(super) enum KeyRef<'a> {
    Name { word: u64, bytes: &'a [u8] },
}
impl KeyRef<'_> {
    pub(super) unsafe fn position(
        self,
        keys: *const crate::array::ArrayHeader,
        count: u32,
    ) -> Option<u32> {
        let Self::Name { word, bytes } = self;
        let hash = crate::object::key_bytes_hash(bytes.as_ptr(), bytes.len());
        // Wide records already own a content-validated shape index. Do not
        // precede its lookup with an unbounded word scan of the same keys.
        if count >= crate::object::KEYS_INDEX_THRESHOLD {
            return crate::object::keys_find_property_slot_by_bytes_resolved_hashed(
                crate::object::ObjectKeys::new(keys as *mut _, count), count, bytes, hash,
            );
        }
        let slots = crate::array::array_elements_ptr(keys);
        if word != 0 {
            if let Some(i) = (0..count).rev().find(|&i| {
                *slots.add(i as usize) == word
                    && !crate::object::key_attrs::entry_is_private(
                        crate::object::key_attrs::keys_entry(keys, i),
                    )
            }) {
                return Some(i);
            }
        }
        crate::object::keys_find_property_slot_by_bytes_resolved_hashed(crate::object::ObjectKeys::new(keys as *mut _, count), count, bytes, hash)
    }
}

pub(super) unsafe fn walk_to_key(
    recv: *const ObjectHeader,
    key: KeyRef<'_>,
    class_first: bool,
    max_depth: usize,
) -> Option<Walk> {
    debug_assert!(max_depth <= WALK_HOPS + 1);
    let mut w = Walk {
        holder: 0,
        holder_shape: LayoutWord(0),
        slot: None,
        hops: NO_HOPS,
        depth: 0,
    };
    let object_prototype = crate::array::object_prototype_addr_if_resolved();
    let mut current = recv;
    for depth in 1..=max_depth {
        // `%Object.prototype%` is an immutable-prototype exotic object: its
        // [[Prototype]] is null for its whole life, whatever its shape's
        // identity word says, so reaching it ends the chain.
        let terminal = depth > 1 && current as usize == object_prototype;
        // `current`'s recorded word, once read for its identity check.
        let mut word = None;
        let pid = if terminal {
            PROTO_ID_NULL
        } else if depth == 1 && class_first {
            // A bare CLASS ShapeId does not pin the registry's live
            // C.prototype. The collecting class-read hit compares that
            // pointer on every use; this walk records it as the first hop.
            crate::object::shapes::PROTO_ID_CLASS
        } else {
            let (pid, w) = admitted_link(current)?;
            word = Some(w);
            pid
        };
        if pid == PROTO_ID_NULL {
            // `current` is the terminal object, and it lacks `name`.
            if depth == 1 {
                return None;
            }
            let (h, sh) = w.hops[depth - 2];
            w.hops[depth - 2] = (0, 0);
            w.holder = h;
            w.holder_shape = LayoutWord(sh);
            w.depth = depth - 1;
            return Some(w);
        }
        let next = if depth == 1 && class_first {
            class_link(recv)?
        } else if pid == PROTO_ID_DEFAULT {
            object_prototype as *const ObjectHeader
        } else {
            match word {
                Some(w) => next_from_word(current, w),
                None => next_prototype(current),
            }
        };
        if next.is_null() || next == current || next == recv || !hop_admitted(next as usize) {
            return None;
        }
        let shape = object_shape_descriptor(next)?;
        // A hop whose layout can have holes holds or lacks its optional keys
        // per object (`key_view`): no walk describes it.
        if !shape.object_kind.is_ordinary_layout()
            || object_shape_stamp(next) == LayoutWord(0)
            || crate::object::key_view::summary_may_have_holes(shape.summary)
        {
            return None;
        }
        let keys = shape.keys as usize as *const crate::array::ArrayHeader;
        if !keys.is_null() {
            if let Some(s) = key.position(keys, shape.logical_key_count) {
                // This exact slot, not another name walk, supplies the data
                // admission proof kept by the existing holder memo. Its
                // ShapeId compare invalidates it when the entry changes.
                if crate::object::key_attrs::key_is_accessor_at(keys, s) {
                    return None;
                }
                let s = holder_slot_word(
                    next as usize,
                    shape.keys as usize as *const crate::ArrayHeader,
                    s,
                    shape.live_inline_slot_count,
                )?;
                w.holder = next as usize;
                w.holder_shape = LayoutWord(object_shape_stamp(next).0);
                w.slot = Some(s);
                w.depth = depth;
                return Some(w);
            }
        }
        if depth == max_depth {
            // One more object would be needed: either the holder or the null
            // link past the last hop.
            if next as usize != object_prototype && admitted_proto_id(next) != Some(PROTO_ID_NULL) {
                return None;
            }
            w.holder = next as usize;
            w.holder_shape = LayoutWord(object_shape_stamp(next).0);
            w.depth = depth;
            return Some(w);
        }
        w.hops[depth - 1] = (next as usize, object_shape_stamp(next).0);
        current = next;
    }
    None
}
