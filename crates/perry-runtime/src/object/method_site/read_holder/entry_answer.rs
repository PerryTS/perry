//! Shared primary and saved holder validation, including receiver-relative slots.
use super::*;

/// The entry's answer (value bits) for a receiver whose PIC token is
/// `token`, or `None` when the entry is empty, names another receiver shape,
/// or any hop or holder word it recorded has moved. Reads site words and
/// object words only: a GC leaf.
///
/// The loaded slot needs no `TAG_HOLE` test, for the reason the emitted MRU
/// hit needs none: every delete is a shape transition (#10826), so a holder
/// whose ShapeId still matches has not had the slot cleared.
#[inline(always)]
pub(crate) unsafe fn entry_answer(c: &PicCache, token: i64) -> Option<u64> {
    primary_entry_answer(c, token, std::ptr::null()).or_else(|| {
        if !class_read::has_site(c) || WORKER_AGENTS_EXIST.load(Ordering::SeqCst) != 0 || token == 0
        {
            return None;
        }
        class_read::holder_answer(c, token)
    })
}

/// Primary answer only. The native front asks saved answers through the
/// existing class/site admission after this fails, keeping monomorphic reads
/// free of a second site-state check.
#[inline(always)]
pub(crate) unsafe fn primary_entry_answer(
    c: &PicCache,
    token: i64,
    recv: *const ObjectHeader,
) -> Option<u64> {
    single_entry_answer(holder_words(c), token, recv)
}

#[inline(always)]
unsafe fn single_entry_answer(
    c: &HolderEntry,
    token: i64,
    recv: *const ObjectHeader,
) -> Option<u64> {
    if WORKER_AGENTS_EXIST.load(Ordering::SeqCst) != 0 {
        return None;
    }
    if token == 0 {
        return None;
    }
    // The common data/stub entry checks its receiver token before decoding
    // kinds. Only a token miss can search the extra ABSENT shapes; keeping
    // that search off the ordinary inherited hit avoids taxing every read.
    if c[HOLDER_RECV] != token {
        // Only a multi-shape entry has further receiver tokens; every other
        // site (a class entry's site among them) leaves on this load.
        if c[HOLDER_KIND] as u64 & HOLDER_MULTI_ABSENT == 0 {
            return None;
        }
        return multi_absent_extra_answer(c, token);
    }
    let kind = c[HOLDER_KIND];
    // A depth-1 data holder is the common inherited-read hit. Its kind is
    // exactly an inline slot number; answer it before the absent, accessor,
    // multi-shape and deeper-hop decoding below.
    if (kind as u64) < u64::from(HOLDER_SLOT_SPILL) {
        let holder = c[HOLDER_OBJ] as usize;
        if shape_word(holder) != c[HOLDER_SHAPE] as u32 {
            return None;
        }
        return Some(slot_bits(holder, kind as u32));
    }
    entry_answer_other(c, kind, recv)
}

/// Share the validator across saved-entry scans without expanding the
/// primary receiver/holder checks into each scan's loop body.
#[cold]
#[inline(never)]
pub(super) unsafe fn saved_entry_answer(
    c: &HolderEntry,
    token: i64,
    recv: *const ObjectHeader,
) -> Option<u64> {
    single_entry_answer(c, token, recv)
}

/// Uncommon entries keep their complete depth/absence validation off the
/// inlined depth-1 data hit, including the class-field read's caller.
#[cold]
#[inline(never)]
unsafe fn entry_answer_other(c: &HolderEntry, kind: i64, recv: *const ObjectHeader) -> Option<u64> {
    if kind as u64 & HOLDER_FUNCTION_BAG != 0 {
        return if recv.is_null() {
            None
        } else {
            function_own::entry_answer(c, recv, c[HOLDER_RECV])
        };
    }
    if kind as u64 & (HOLDER_ACCESSOR | HOLDER_MULTI_ABSENT) != 0 {
        if kind as u64 & HOLDER_ACCESSOR != 0 {
            return None;
        }
        return multi_answer(c, kind as u64);
    }
    let (depth, absent, slot) = if kind >= 0 {
        (1, kind == HOLDER_ABSENT_DEPTH1, kind as u32)
    } else {
        let k = kind as u64;
        (
            ((k >> HOLDER_DEPTH_SHIFT) & 0xF) as usize,
            k & HOLDER_ABSENT_BIT != 0,
            k as u32,
        )
    };
    let hop_shapes = c[HOLDER_HOP_SHAPES] as u64;
    let shape_words = [
        hop_shapes as u32,
        (hop_shapes >> 32) as u32,
        (c[HOLDER_SHAPE] as u64 >> 32) as u32,
    ];
    for i in 0..depth.saturating_sub(1).min(HOLDER_MAX_DEPTH - 1) {
        if shape_word(c[HOLDER_HOPS + i] as usize) != shape_words[i] {
            return None;
        }
    }
    let holder = c[HOLDER_OBJ] as usize;
    if shape_word(holder) != c[HOLDER_SHAPE] as u32 {
        return None;
    }
    if absent {
        return Some(crate::value::TAG_UNDEFINED);
    }
    holder_slot_value(holder, slot)
}

/// A second through tenth receiver shape of a multi-shape depth-1 entry is a
/// rare path relative to one-token data hits. It shares the holder (the
/// terminal object for an absent entry) but must still prove the entry is
/// live and the holder's ShapeId has not changed.
#[cold]
#[inline(never)]
unsafe fn multi_absent_extra_answer(c: &HolderEntry, token: i64) -> Option<u64> {
    let kind = c[HOLDER_KIND] as u64;
    if c[HOLDER_RECV] == 0
        || kind & HOLDER_MULTI_ABSENT == 0
        || kind & HOLDER_ACCESSOR != 0
        || !(0..MULTI_ABSENT_EXTRA_IDS).any(|i| multi_absent_id(c, i) == token as u32)
    {
        return None;
    }
    multi_answer(c, kind)
}

/// A multi-shape depth-1 entry's answer once a receiver token matched: the
/// holder's ShapeId, then `undefined` or the holder's slot. Every receiver
/// shape it names was admitted on its own walk and getter confirmation, with
/// this holder and holder ShapeId and (for data) this slot word.
#[inline]
unsafe fn multi_answer(c: &HolderEntry, kind: u64) -> Option<u64> {
    let holder = c[HOLDER_OBJ] as usize;
    if shape_word(holder) != c[HOLDER_SHAPE] as u32 {
        return None;
    }
    if kind & HOLDER_ABSENT_BIT != 0 {
        return Some(crate::value::TAG_UNDEFINED);
    }
    holder_slot_value(holder, multi_slot(kind))
}
