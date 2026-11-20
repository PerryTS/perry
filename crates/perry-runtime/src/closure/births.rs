//! Data-driven closure births. A run of fresh closures that differ only in
//! constants (body info, which capture words, where the closure goes) is
//! compiled to a read-only table of [`ClosureBirth`] records plus one call
//! that walks it, instead of one inline birth per closure.
//!
//! Each record makes exactly the closure the inline lowering makes
//! (`js_closure_alloc_small` / `js_closure_alloc_fixed`): a fresh function
//! object of the body's birth shape, its captures copied from the caller's
//! source words. Records are walked in order, so the closures are born in
//! source order.
//!
//! Inputs are compiler-owned static data plus the caller's source words (the
//! capture words it would have passed inline). No user code runs while a
//! table is walked, so GC suppression keeps the source words and the
//! closures born so far valid, as for the literal descriptor.

use super::{ClosureHeader, JsFunctionInfo};

const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

/// Keep in sync with perry-codegen::closure_births::BIRTH_TYPE.
#[repr(C)]
pub struct ClosureBirth {
    info: *const JsFunctionInfo,
    /// The header's capture-count word, flags included.
    capture_count: u32,
    /// Index of the first capture's source index in the capture index table;
    /// the record has `ordinary_capture_count(capture_count)` of them.
    first_capture: u32,
    /// For a scope run: the source index of the scope object the closure is
    /// stored into, and the slot index in it. Unused for a descriptor node.
    dest_source: u32,
    dest_slot: u32,
}

/// The source words and capture index table one table walk reads.
#[derive(Clone, Copy)]
pub(crate) struct BirthSources<'a> {
    pub(crate) capture_indices: &'a [u32],
    pub(crate) words: &'a [u64],
}

impl BirthSources<'_> {
    /// # Safety
    /// `indices` points at `index_count` u32s and `words` at `word_count`
    /// u64s, both live for `'a`.
    pub(crate) unsafe fn from_raw<'a>(
        indices: *const u32,
        index_count: u32,
        words: *const u64,
        word_count: u32,
    ) -> BirthSources<'a> {
        BirthSources {
            capture_indices: if index_count != 0 && !indices.is_null() {
                std::slice::from_raw_parts(indices, index_count as usize)
            } else {
                &[]
            },
            words: if word_count != 0 && !words.is_null() {
                std::slice::from_raw_parts(words, word_count as usize)
            } else {
                &[]
            },
        }
    }

    fn word(&self, source: u32) -> Option<u64> {
        self.words.get(source as usize).copied()
    }
}

/// Bear `birth`'s closure. `None` on a malformed record.
pub(crate) fn bear(birth: &ClosureBirth, sources: BirthSources<'_>) -> Option<*mut ClosureHeader> {
    let count = super::alloc::ordinary_capture_count(birth.capture_count);
    let first = birth.first_capture as usize;
    let indices = sources
        .capture_indices
        .get(first..first.checked_add(count)?)?;
    let mut words = [0u64; 8];
    let mut spilled = Vec::new();
    let buffer: &mut [u64] = if count <= words.len() {
        &mut words[..count]
    } else {
        spilled.resize(count, 0);
        &mut spilled
    };
    for (word, &source) in buffer.iter_mut().zip(indices) {
        *word = sources.word(source)?;
    }
    let shape_id = super::shape::birth_shape_for_body(birth.info);
    let header = (u64::from(shape_id.0) << 32) | u64::from(birth.capture_count);
    Some(super::alloc::js_closure_alloc_fixed(
        birth.info,
        header,
        if count == 0 {
            std::ptr::null()
        } else {
            buffer.as_ptr()
        },
    ))
}

/// Bear `count` closures and store each into its scope slot,
/// `dest_slot` of the scope object in source `dest_source`, with the write
/// barrier a scope store takes (`scope_env::access::write_bits`).
#[no_mangle]
pub extern "C" fn js_closure_births_to_scope(
    births: *const ClosureBirth,
    count: u32,
    capture_indices: *const u32,
    index_count: u32,
    words: *const u64,
    word_count: u32,
) {
    if births.is_null() || count == 0 {
        return;
    }
    let _suppress = crate::gc::GcSuppressScope::new();
    let births = unsafe { std::slice::from_raw_parts(births, count as usize) };
    let sources =
        unsafe { BirthSources::from_raw(capture_indices, index_count, words, word_count) };
    for birth in births {
        let Some(scope) = sources.word(birth.dest_source) else {
            return;
        };
        let base = scope & POINTER_MASK;
        if base == 0 {
            return;
        }
        let Some(closure) = bear(birth, sources) else {
            return;
        };
        let bits = crate::value::JSValue::pointer(closure as *const u8).bits();
        let slot = base + 8 * u64::from(birth.dest_slot);
        // GC_STORE_AUDIT(BARRIERED): the scope slot store codegen emits for
        // the binding's declaration, followed by its slot barrier.
        unsafe { (slot as *mut u64).write(bits) };
        crate::gc::js_write_barrier_slot_validated_parent(base, slot, bits);
    }
}

#[cfg(test)]
#[path = "births_tests.rs"]
mod tests;
