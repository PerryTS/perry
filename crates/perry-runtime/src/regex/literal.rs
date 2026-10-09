//! A literal site owns immutable matcher data, never a RegExp instance.
//! Workers must not read or write the primary agent's image-global word.
//! The word uses the existing mutable global-root contract, so both its
//! RegExpData edge and the data's string/program edges survive evacuation.
use super::{RegExpData, RegExpHeader};
use crate::gc::RuntimeHandleScope;
use crate::string::StringHeader;

/// `site_word` is an immortal, aligned, zero-initialized codegen global of two
/// words unique to this literal: word 0 the data root, word 1 the birth's
/// header word. Only the primary JS agent accesses it. On first use it
/// becomes an existing global root; subsequent evaluations allocate a fresh
/// ordinary object with its own lastIndex around the same immutable data.
///
/// Every literal site of a program calls this one entry, so a site is the
/// call and its operands. The common evaluation is [`inline_birth`]: a bump of
/// the inline arena, the way an emitted `new` allocates.
#[no_mangle]
pub extern "C" fn js_regexp_literal(
    pattern: *const StringHeader,
    flags: *const StringHeader,
    site_word: i64,
) -> *mut RegExpHeader {
    let word = site_word as *mut u64;
    // SAFETY: `word` is the site's two-word global (above).
    let born = unsafe { inline_birth(word) };
    if !born.is_null() {
        return born;
    }
    literal_birth(pattern, flags, word)
}

/// GcHeader + ObjectHeader + the two slots (matcher data, lastIndex).
const BIRTH_BYTES: usize = crate::gc::GC_HEADER_SIZE + std::mem::size_of::<RegExpHeader>() + 16;
const _: () = assert!(
    BIRTH_BYTES % 8 == 0,
    "the inline arena offset stays 8-aligned"
);

/// A fresh RegExp born in the inline arena around word 0's data, under the
/// header word the site's first evaluation published (word 1: class id and
/// birth ShapeId, a shape whose two lanes are `Any`); or null when this
/// evaluation must take [`literal_birth`]: word 1 unpublished (the first
/// evaluation), a worker exists (the words belong to the primary agent), or
/// the open block has no room.
///
/// Nothing here collects, so word 0 read before the slot store is the data's
/// current address. The newborn is in the nursery (no remembered-set entry)
/// and is seeded for an active mark only after both slots hold their values:
/// the birth protocol of an emitted `new` (`perry-codegen/src/expr/inline_birth.rs`).
///
/// # Safety
/// `word` is a literal site's two-word global.
#[inline(always)]
unsafe fn inline_birth(word: *mut u64) -> *mut RegExpHeader {
    // The worker gate first: a worker reads neither word.
    if crate::object::method_site::WORKER_AGENTS_EXIST.load(std::sync::atomic::Ordering::SeqCst)
        != 0
    {
        return std::ptr::null_mut();
    }
    let header_word = word.add(1).read();
    if header_word == 0 {
        return std::ptr::null_mut();
    }
    let mut state = crate::arena::hot_inline_state();
    if (*state).data.is_null() || (*state).birth_flags.is_null() {
        state = crate::arena::js_inline_arena_state();
    }
    let offset = (*state).offset;
    let end = offset + BIRTH_BYTES;
    if end > (*state).size {
        return std::ptr::null_mut();
    }
    // GC_STORE_AUDIT(INIT): inline arena bump offset is allocator metadata, not a JS heap edge.
    (*state).offset = end;
    let raw = (*state).data.add(offset);
    let birth = std::ptr::read_volatile((*state).birth_flags);
    // GC_STORE_AUDIT(INIT): the header of freshly bumped, unpublished storage.
    raw.cast::<crate::gc::GcHeader>()
        .write(crate::gc::GcHeader {
            obj_type: crate::gc::GC_TYPE_OBJECT,
            gc_flags: crate::gc::GC_FLAG_ARENA | birth,
            _reserved: crate::gc::OBJ_FLAG_PLAIN_ORDINARY,
            size: BIRTH_BYTES as u32,
        });
    let re = raw.add(crate::gc::GC_HEADER_SIZE).cast::<RegExpHeader>();
    // GC_STORE_AUDIT(INIT): class id and birth ShapeId, never a heap reference.
    re.cast::<u64>().write(header_word);
    // GC_STORE_AUDIT(INIT): a fresh RegExp has no per-object meta record.
    (*re).meta = std::ptr::null_mut();
    let slots = re.add(1).cast::<u64>();
    // GC_STORE_AUDIT(INIT): newborn nursery RegExp's matcher slot; the seed below covers marking.
    slots.write(word.read());
    // GC_STORE_AUDIT(INIT): newborn RegExp's lastIndex, the Number +0.
    slots.add(1).write(0.0f64.to_bits());
    if birth != 0 {
        crate::gc::js_gc_note_black_birth(raw.cast(), (*state).birth_seeds);
    }
    re
}

/// The runtime birth: the first evaluation (which publishes both words), any
/// agent while workers exist, and a full block.
#[inline(never)]
fn literal_birth(
    pattern: *const StringHeader,
    flags: *const StringHeader,
    word: *mut u64,
) -> *mut RegExpHeader {
    if crate::agent::current_agent() != crate::agent::PRIMARY_AGENT {
        return super::perex_api::finish(super::perex_construct::new(pattern, flags));
    }
    if unsafe { word.read() } == 0 {
        literal_miss(pattern, flags, word);
    }
    // The word is a registered global root: after the birth's allocation it
    // holds the data cell's current address.
    let re = super::instance::new(|| unsafe {
        (word.read() & crate::value::POINTER_MASK) as *const RegExpData
    });
    // Word 1: the header word [`inline_birth`] stamps. Published only after
    // word 0, and only for a birth shape whose slots that birth writes raw;
    // the shape is then externally carried (never pruned).
    unsafe {
        let header_word = word.add(1);
        if header_word.read() == 0 {
            if let Some(image) = super::instance::inline_birth_header_word(re) {
                // GC_STORE_AUDIT(POINTER_FREE): a class id and ShapeId, never a heap reference.
                header_word.write(image);
            }
        }
    }
    re
}

#[cold]
#[inline(never)]
fn literal_miss(pattern: *const StringHeader, flags: *const StringHeader, word: *mut u64) {
    let scope = RuntimeHandleScope::new();
    let data = super::perex_api::finish(super::perex_construct::new_data(&scope, pattern, flags));
    data.with_mut_ptr::<RegExpData, _>(|data| unsafe {
        // The existing root-store barrier admits publication during an
        // incremental cycle. Registration exposes the mutable word to both
        // marking and evacuation rewriting; neither operation collects here.
        crate::gc::runtime_store_root_nanbox_f64_raw_slot(
            word.cast(),
            crate::value::js_nanbox_pointer(data as i64),
        );
        crate::gc::js_gc_register_global_root(word as i64);
    });
}

#[cfg(test)]
mod tests;
