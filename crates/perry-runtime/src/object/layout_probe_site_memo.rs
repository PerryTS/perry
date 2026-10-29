//! Per-site answers derived exclusively from the shapes on a bounded chain.
//! The entries contain no heap addresses: each use follows the live shape
//! links and reads the current holder slot, including its current getter pair.
//!
//! Only a collecting entrance writes a memo. Named leaves retain stateless
//! data handling and ask the memo after an own miss. Their four-argument ABI
//! passes the memo instead of a hash: short own lists need neither; other
//! paths load the atom's existing creation hash. Keyed leaves stay stateless.
use super::*;
use crate::codegen_abi::LayoutWord;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

const PAIRS: usize = 5;
const NEXT: u32 = u32::MAX;
const ABSENT: u32 = u32::MAX - 1;
/// The receiver's shape has no memo answer at this site (its chain is not
/// one the shapes prove): the collecting entrance takes the complete read
/// without deriving again. A negative answer, checked by the same ShapeId.
const DECLINE: u32 = u32::MAX - 2;
const ACCESSOR: u32 = 1 << 31;
const SPILL: u32 = 1 << 30;
const SLOT_MASK: u32 = SPILL - 1;

/// Exactly five (LayoutWord, slot) pairs. Zero shape means an empty answer.
/// Shared integer storage, never indexed through a registry. A LayoutWord is
/// agent-local storage of a row, but within one image it names one shape in
/// every agent: static rows are the image's compile-time ordinals, dynamic
/// rows come from process-unique chunks. A compiled site's memo lives in its
/// image's data; a runtime-crate memo is shared by every image of the process,
/// whose static rows N..S-1 differ, so it serves only the primary image
/// (`class_image::process_shared_memo`). A hit also needs a live receiver
/// carrying the word, so the row it reads is published in this agent. Each
/// atomic word is a complete proof for this fixed key; a racing writer can
/// cause a shape mismatch, never a torn (shape, slot). Every continuation
/// revalidates the live link and next shape. There is no whole-memo validity
/// bit, epoch, or cached heap address.
#[repr(C)]
pub struct ReadSiteMemo {
    pairs: [AtomicU64; PAIRS],
}
impl ReadSiteMemo {
    pub(crate) const EMPTY: Self = Self {
        pairs: [const { AtomicU64::new(0) }; PAIRS],
    };

    fn publish(&self, pairs: &[u64; PAIRS]) {
        for (dst, &src) in self.pairs.iter().zip(pairs).rev() {
            dst.store(src, Relaxed);
        }
    }

    fn snapshot(&self) -> [u64; PAIRS] {
        std::array::from_fn(|i| self.pairs[i].load(Relaxed))
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Answer {
    Data(u64),
    // The live pair word, decoded only by the collecting entrance.
    // Keeping a full Accessor here made even data hits return 56 bytes
    // through a hidden sret argument and a stack frame.
    Getter(u64),
}

#[inline(always)]
unsafe fn ordinary_word(obj: *const ObjectHeader) -> Option<u32> {
    let h = crate::gc::header_from_trusted_user_ptr(obj.cast());
    if !crate::gc::gc_type_is_object((*h).obj_type)
        || (*h).gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
        || (*h)._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO != 0
    {
        return None;
    }
    let meta = crate::object::header::object_meta_if_present(obj as *mut _, (*h).obj_type);
    if !meta.is_null()
        && (*meta).flags
            & (crate::object::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER
                | crate::object::OBJECT_META_FLAG_NATIVE_ALIAS)
            != 0
    {
        return None;
    }
    let word = (*obj).parent_class_id;
    // An overflow's SHADOW_BIT deliberately prevents a site comparison.
    crate::object::shapes::is_site_matchable_layout_word(word).then_some(word)
}

#[inline(always)]
unsafe fn slot_answer(obj: *const ObjectHeader, slot: u32) -> Option<Answer> {
    if slot == ABSENT {
        return Some(Answer::Data(crate::value::TAG_UNDEFINED));
    }
    let index = slot & SLOT_MASK;
    let bits = if slot & SPILL != 0 {
        crate::object::spill::spill_get_present(obj as usize, index as usize)?
    } else {
        *((obj as *const u8).add(std::mem::size_of::<ObjectHeader>() + index as usize * 8)
            as *const u64)
    };
    if bits == TAG_HOLE {
        return None;
    }
    if slot & ACCESSOR != 0 {
        Some(Answer::Getter(bits))
    } else {
        Some(Answer::Data(bits))
    }
}

/// The complete O(1) proof on every use. No key lookup, allocation or user
/// code. Each continuation's shape proves absence and supplies its live link.
#[inline]
pub(crate) unsafe fn lookup(
    dir: *const u8,
    mut obj: *const ObjectHeader,
    memo: &ReadSiteMemo,
) -> Option<Answer> {
    for entry in &memo.pairs {
        let pair = entry.load(Relaxed);
        let shape = (pair >> 32) as u32;
        if shape == 0 || (*obj).parent_class_id != shape || ordinary_word(obj)? != shape {
            return None;
        }
        let slot = pair as u32;
        if slot == DECLINE {
            return None;
        }
        if slot != NEXT && slot != ABSENT {
            if let Some(answer) = slot_answer(obj, slot) {
                return Some(answer);
            }
            // A data hole is absence on this object, not absence proved by
            // the shape. Continue only through the live link and verified
            // tail. An accessor hole retains the complete getter's rules.
            if slot & ACCESSOR != 0 {
                return None;
            }
        }
        let r = record_of(dir, LayoutWord(shape));
        if r.is_null() {
            return None;
        }
        if slot == ABSENT {
            return matches!(link_of(dir, obj, r), Next::End)
                .then_some(Answer::Data(crate::value::TAG_UNDEFINED));
        }
        obj = match link_of(dir, obj, r) {
            Next::Object(next) => next,
            Next::End => return Some(Answer::Data(crate::value::TAG_UNDEFINED)),
            Next::Unknown => return None,
        };
    }
    None
}

/// Has this memo found that a receiver of `obj`'s shape has no memo answer?
#[inline(always)]
unsafe fn declined(obj: *const ObjectHeader, memo: &ReadSiteMemo) -> bool {
    memo.pairs[0].load(Relaxed) == (u64::from((*obj).parent_class_id) << 32) | u64::from(DECLINE)
}

/// Remember that a receiver of `obj`'s shape has no memo answer, unless the
/// memo already holds a positive answer for another shape.
#[inline]
unsafe fn decline(obj: *const ObjectHeader, memo: &ReadSiteMemo) {
    let word = (*obj).parent_class_id;
    let held = memo.pairs[0].load(Relaxed);
    if crate::object::shapes::is_site_matchable_layout_word(word)
        && (held == 0 || held as u32 == DECLINE)
    {
        // A decline uses only word zero. Do not clear a racing positive
        // proof's tail; the CAS also preserves a concurrently published proof.
        let _ = memo.pairs[0].compare_exchange(
            held,
            (u64::from(word) << 32) | u64::from(DECLINE),
            Relaxed,
            Relaxed,
        );
    }
}

/// A collecting entrance's answer for a fixed key: the memo's proof, else a
/// derivation from the live chain, unless this receiver shape already
/// declined. A derivation that declines is remembered as [`DECLINE`].
#[inline(always)]
pub(crate) unsafe fn collect(
    dir: *const u8,
    obj: *const ObjectHeader,
    key_bits: u64,
    hash: u64,
    memo: &ReadSiteMemo,
) -> Option<Answer> {
    if let Some(answer) = lookup(dir, obj, memo) {
        return Some(answer);
    }
    if declined(obj, memo) {
        return None;
    }
    let answer = prime(dir, obj, key_bits, hash, memo);
    if answer.is_none() {
        decline(obj, memo);
    }
    answer
}

/// Derive the memo without running user code. Data holes retain a guarded
/// prototype tail; overflow,
/// dictionaries, exotics and chains longer than the fixed bound decline.
#[inline(never)]
pub(crate) unsafe fn prime(
    dir: *const u8,
    mut obj: *const ObjectHeader,
    key_bits: u64,
    hash: u64,
    memo: &ReadSiteMemo,
) -> Option<Answer> {
    let mut learned = [0u64; PAIRS];
    for i in 0..PAIRS {
        let shape = ordinary_word(obj)?;
        let r = record_of(dir, LayoutWord(shape));
        if r.is_null()
            || !(*r).present()
            || !positional_layout(&*r)
            || !matches!(
                (*r).object_kind(),
                super::super::ShapeObjectKind::Ordinary
                    | super::super::ShapeObjectKind::OrdinaryUnmarked
            )
            || !walks_from(obj, r)
        {
            return None;
        }
        let prefix = u64::from(shape) << 32;
        match own_entry(r, key_bits, hash) {
            Ok((pos, _)) => {
                let keys = (*r).keys as usize as *const crate::array::ArrayHeader;
                let attrs = crate::object::key_attrs::keys_entry(keys, pos);
                if attrs & crate::object::key_attrs::ENTRY_PRIVATE != 0 {
                    return None;
                }
                let slot = crate::object::key_attrs::key_storage_slot(keys, pos);
                if slot >= SLOT_MASK - 1 {
                    return None;
                }
                let slot =
                    slot | if slot < (*r).live_inline_slot_count {
                        0
                    } else {
                        SPILL
                    } | if attrs & crate::object::key_attrs::ENTRY_ACCESSOR_MASK != 0 {
                        ACCESSOR
                    } else {
                        0
                    };
                learned[i] = prefix | u64::from(slot);
                if let Some(answer) = slot_answer(obj, slot) {
                    memo.publish(&learned);
                    return Some(answer);
                }
                if slot & ACCESSOR != 0 {
                    return None;
                }
                // Keep the data slot itself in the entry: every use must
                // re-read its live presence before trusting the tail.
            }
            Err(false) => return None,
            Err(true) => {}
        }
        match link_of(dir, obj, r) {
            Next::End => {
                if learned[i] == 0 {
                    learned[i] = prefix | u64::from(ABSENT);
                }
                memo.publish(&learned);
                return Some(Answer::Data(crate::value::TAG_UNDEFINED));
            }
            Next::Object(next) => {
                if learned[i] == 0 {
                    learned[i] = prefix | u64::from(NEXT);
                }
                obj = next;
            }
            Next::Unknown => return None,
        }
    }
    None
}

/// The leaf's question after an own miss: does the memo, whose first
/// ShapeId is the receiver's, answer? `None` continues the stateless probe.
#[inline(always)]
unsafe fn leaf_answer(
    dir: *const u8,
    obj: *const ObjectHeader,
    key_bits: u64,
    memo: *const ReadSiteMemo,
) -> Option<f64> {
    Some(match lookup(dir, obj, &*memo)? {
        Answer::Data(bits) => verify::answer(obj as usize, key_bits, f64::from_bits(bits)),
        // The collecting entrance reloads and calls the live getter.
        Answer::Getter(_) => hole(),
    })
}

/// An indexed atom comparison collided. The existing shape/slot proof
/// may already answer the whole read; otherwise resume the index exactly
/// after its first bucket. This does not infer absence from a collision.
#[inline(never)]
pub(super) unsafe fn collision<const ABSENT_OK: bool, const LAZY_HASH: bool>(
    dir: *const u8,
    obj: *const ObjectHeader,
    key_bits: u64,
    hash: u64,
    memo: *const ReadSiteMemo,
) -> f64 {
    let pair = (*memo).pairs[0].load(Relaxed);
    if (pair >> 32) as u32 == (*obj).parent_class_id && pair as u32 != DECLINE {
        return collision_matched::<ABSENT_OK, LAZY_HASH>(dir, obj, key_bits, hash, memo);
    }
    indexed::continued::<ABSENT_OK, true, LAZY_HASH>(dir, obj, key_bits, hash, memo)
}

#[inline(never)]
unsafe fn collision_matched<const ABSENT_OK: bool, const LAZY_HASH: bool>(
    dir: *const u8,
    obj: *const ObjectHeader,
    key_bits: u64,
    hash: u64,
    memo: *const ReadSiteMemo,
) -> f64 {
    if let Some(v) = leaf_answer(dir, obj, key_bits, memo) {
        return v;
    }
    // A matched but unanswered proof may have a live hole or an unproved
    // link. The continuation reconstructs its cursor after the proof attempt.
    indexed::continued::<ABSENT_OK, true, LAZY_HASH>(dir, obj, key_bits, hash, memo)
}

/// The own list proved the key absent.
#[inline(never)]
pub(super) unsafe fn miss_absent<const ABSENT_OK: bool, const LAZY_HASH: bool>(
    dir: *const u8,
    obj: *const ObjectHeader,
    key_bits: u64,
    hash: u64,
    memo: *const ReadSiteMemo,
) -> f64 {
    let pair = (*memo).pairs[0].load(Relaxed);
    if (pair >> 32) as u32 == (*obj).parent_class_id {
        return miss_absent_matched::<ABSENT_OK, LAZY_HASH>(dir, obj, key_bits, hash, memo);
    }
    let hash = site_key_hash::<LAZY_HASH>(key_bits, hash);
    absent_from::<ABSENT_OK>(dir, obj, key_bits, hash)
}

#[inline(never)]
unsafe fn miss_absent_matched<const ABSENT_OK: bool, const LAZY_HASH: bool>(
    dir: *const u8,
    obj: *const ObjectHeader,
    key_bits: u64,
    hash: u64,
    memo: *const ReadSiteMemo,
) -> f64 {
    if let Some(v) = leaf_answer(dir, obj, key_bits, memo) {
        return v;
    }
    let hash = site_key_hash::<LAZY_HASH>(key_bits, hash);
    absent_from::<ABSENT_OK>(dir, obj, key_bits, hash)
}

/// The receiver's record is not derived, or is checked (accessor/private).
#[inline(never)]
pub(super) unsafe fn miss_underived<const ABSENT_OK: bool, const LAZY_HASH: bool>(
    dir: *const u8,
    obj: *const ObjectHeader,
    key_bits: u64,
    hash: u64,
    memo: *const ReadSiteMemo,
) -> f64 {
    let pair = (*memo).pairs[0].load(Relaxed);
    if (pair >> 32) as u32 == (*obj).parent_class_id {
        return miss_underived_matched::<ABSENT_OK, LAZY_HASH>(dir, obj, key_bits, hash, memo);
    }
    let hash = site_key_hash::<LAZY_HASH>(key_bits, hash);
    let word = (*obj).parent_class_id;
    probe_get_underived::<ABSENT_OK>(
        dir,
        obj,
        LayoutWord(word),
        record_of(dir, LayoutWord(word)),
        key_bits,
        hash,
    )
}

#[inline(never)]
unsafe fn miss_underived_matched<const ABSENT_OK: bool, const LAZY_HASH: bool>(
    dir: *const u8,
    obj: *const ObjectHeader,
    key_bits: u64,
    hash: u64,
    memo: *const ReadSiteMemo,
) -> f64 {
    if let Some(v) = leaf_answer(dir, obj, key_bits, memo) {
        return v;
    }
    let hash = site_key_hash::<LAZY_HASH>(key_bits, hash);
    let word = (*obj).parent_class_id;
    probe_get_underived::<ABSENT_OK>(
        dir,
        obj,
        LayoutWord(word),
        record_of(dir, LayoutWord(word)),
        key_bits,
        hash,
    )
}

/// The own slot at position `pos` read a hole.
#[inline(never)]
pub(super) unsafe fn miss_hole<const ABSENT_OK: bool, const LAZY_HASH: bool>(
    dir: *const u8,
    obj: *const ObjectHeader,
    pos: u32,
    key_bits: u64,
    hash: u64,
    memo: *const ReadSiteMemo,
) -> f64 {
    let pair = (*memo).pairs[0].load(Relaxed);
    if (pair >> 32) as u32 == (*obj).parent_class_id {
        return miss_hole_matched::<ABSENT_OK, LAZY_HASH>(dir, obj, pos, key_bits, hash, memo);
    }
    let hash = site_key_hash::<LAZY_HASH>(key_bits, hash);
    own_hole::<ABSENT_OK>(
        dir,
        obj,
        record_of(dir, LayoutWord((*obj).parent_class_id)),
        pos,
        key_bits,
        hash,
    )
}

#[inline(never)]
unsafe fn miss_hole_matched<const ABSENT_OK: bool, const LAZY_HASH: bool>(
    dir: *const u8,
    obj: *const ObjectHeader,
    pos: u32,
    key_bits: u64,
    hash: u64,
    memo: *const ReadSiteMemo,
) -> f64 {
    if let Some(v) = leaf_answer(dir, obj, key_bits, memo) {
        return v;
    }
    let hash = site_key_hash::<LAZY_HASH>(key_bits, hash);
    own_hole::<ABSENT_OK>(
        dir,
        obj,
        record_of(dir, LayoutWord((*obj).parent_class_id)),
        pos,
        key_bits,
        hash,
    )
}

/// Four-argument memo front. Own hits do not read the memo, and short own
/// lists need no hash. Other data reads retain the stateless probe behavior.
/// # Safety
/// As perry_probe_get_site, but key_bits must be a property STRING atom.
#[no_mangle]
pub unsafe extern "C" fn perry_probe_get_memo(
    dir: *const u8,
    obj_biased: i64,
    key_bits: u64,
    memo: *const ReadSiteMemo,
) -> f64 {
    probe_get_at::<true, true, true>(dir, obj_biased, key_bits, 0, memo)
}

/// Boxed four-argument fixed-name memo front.
/// # Safety
/// As perry_probe_get_value_site, but the key must be a property STRING atom.
#[no_mangle]
pub unsafe extern "C" fn perry_probe_get_value_memo(
    dir: *const u8,
    obj_bits: u64,
    key_bits: u64,
    memo: *const ReadSiteMemo,
) -> f64 {
    if obj_bits & TAG_MASK != POINTER_TAG {
        return hole();
    }
    let addr = (obj_bits & POINTER_MASK) as usize;
    if addr < perry_abi::RECEIVER_HANDLE_FLOOR {
        return hole();
    }
    perry_probe_get_memo(
        dir,
        (addr - perry_abi::RECEIVER_HANDLE_FLOOR) as i64,
        key_bits,
        memo,
    )
}

/// The fixed-name leaf of a site that owns a memo: [`perry_probe_get`] with
/// the memo asked after an own miss. A GC leaf; it never writes the memo.
/// # Safety
/// As [`perry_probe_get`]; `memo` is this site's shared atomic storage.
#[no_mangle]
pub unsafe extern "C" fn perry_probe_get_site(
    dir: *const u8,
    obj_biased: i64,
    key_bits: u64,
    hash: u64,
    memo: *const ReadSiteMemo,
) -> f64 {
    probe_get_at::<true, true, false>(dir, obj_biased, key_bits, hash, memo)
}

/// [`perry_probe_get_value`] for an outlined or generator site that owns a
/// memo.
/// # Safety
/// As [`perry_probe_get_value`]; `memo` is this site's shared atomic storage.
#[no_mangle]
pub unsafe extern "C" fn perry_probe_get_value_site(
    dir: *const u8,
    obj_bits: u64,
    key_bits: u64,
    hash: u64,
    memo: *const ReadSiteMemo,
) -> f64 {
    if obj_bits & TAG_MASK != POINTER_TAG {
        return hole();
    }
    let payload = (obj_bits & POINTER_MASK) as usize;
    if payload < perry_abi::RECEIVER_HANDLE_FLOOR {
        return hole();
    }
    perry_probe_get_site(
        dir,
        (payload - perry_abi::RECEIVER_HANDLE_FLOOR) as i64,
        key_bits,
        hash,
        memo,
    )
}

#[repr(C)]
struct KeyedWay {
    // Set once from zero to SSO content, never reassigned. Thus every pair
    // in this way proves the same key even while another agent publishes it.
    key: AtomicU64,
    memo: ReadSiteMemo,
}

/// Four shape answers for short string keys, each way bound once on use.
/// The key is CONTENT bits, never a GC address or a cached program value.
#[repr(C)]
pub struct KeyedSiteMemo {
    ways: [KeyedWay; 4],
}

#[inline]
unsafe fn short_key(bits: u64) -> Option<u64> {
    if bits & TAG_MASK == crate::value::SHORT_STRING_TAG {
        return Some(bits);
    }
    if bits & TAG_MASK == STRING_TAG {
        return crate::string::short_ascii_sso_bits(
            (bits & POINTER_MASK) as *const crate::StringHeader,
        );
    }
    None
}

pub(crate) unsafe fn keyed_answer(
    dir: *const u8,
    obj_bits: u64,
    key_bits: u64,
    memo: &KeyedSiteMemo,
) -> Option<Answer> {
    if obj_bits & TAG_MASK != POINTER_TAG {
        return None;
    }
    let addr = (obj_bits & POINTER_MASK) as usize;
    if addr < perry_abi::RECEIVER_HANDLE_FLOOR {
        return None;
    }
    let key = short_key(key_bits)?;
    let obj = addr as *const ObjectHeader;
    if let Some(way) = memo.ways.iter().find(|way| way.key.load(Relaxed) == key) {
        if let Some(answer) = lookup(dir, obj, &way.memo) {
            return Some(answer);
        }
        if declined(obj, &way.memo) {
            return None;
        }
        let (canonical, hash) = computed_key(key_bits)?;
        return collect(dir, obj, canonical, hash, &way.memo);
    }
    if memo.ways.iter().all(|way| way.key.load(Relaxed) != 0) {
        return None;
    }
    let (canonical, hash) = computed_key(key_bits)?;
    let learned = ReadSiteMemo::EMPTY;
    let answer = collect(dir, obj, canonical, hash, &learned);
    let pairs = learned.snapshot();
    // An absent key has no holder slot to derive its content from. Serve
    // that fresh proof without retaining its key, as the stateless path does.
    if pairs.iter().any(|&pair| pair != 0 && pair as u32 == ABSENT) {
        return answer;
    }
    if pairs[0] == 0 {
        return answer;
    }
    // Claim only an empty way: replacing a key would let another reader
    // observe a proof for a different key. A full site uses the complete read.
    if let Some(way) =
        memo.ways.iter().find(
            |way| match way.key.compare_exchange(0, key, Relaxed, Relaxed) {
                Ok(_) => true,
                Err(found) => found == key,
            },
        )
    {
        way.memo.publish(&pairs);
    }
    answer
}

#[cfg(test)]
#[path = "layout_probe_site_memo_tests.rs"]
mod tests;
