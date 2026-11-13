//! Completion of compiler-emitted canonical key lists. The compiler emits a
//! descriptor's list as an image cell; Apple pointer tags and
//! process-lifetime symbols are resolved once, before publication in any
//! agent's trie. The runtime defines no list of its own.
use super::layout_desc::PerryLayoutKey;
use crate::array::ArrayHeader;

#[cfg(all(target_vendor = "apple", target_pointer_width = "64"))]
pub(crate) const fn has_attributes(keys: &[PerryLayoutKey]) -> bool {
    let mut i = 0;
    while i < keys.len() {
        if keys[i].attrs != 0 || keys[i].slot as usize != i {
            return true;
        }
        i += 1;
    }
    false
}

/// Complete only relocations the target cannot express. A compare/exchange
/// performs at most one write per slot; once published, every agent sees the
/// same immutable words. Module-local symbols deliberately have no image list.
pub(crate) unsafe fn prepare(keys: &[PerryLayoutKey], cell: *mut ArrayHeader) -> bool {
    prepare_with(keys, cell, |key| super::layout_desc::layout_key_word(key, &[], &mut 0))
}

/// [`prepare`] for a lazy (realm image) row: its atoms name no registered
/// symbol (`layout_desc::image_key_word`).
pub(crate) unsafe fn prepare_lazy(keys: &[PerryLayoutKey], cell: *mut ArrayHeader) -> bool {
    prepare_with(keys, cell, |key| super::layout_desc::image_key_word(key))
}

#[inline(always)]
unsafe fn prepare_with(
    keys: &[PerryLayoutKey],
    cell: *mut ArrayHeader,
    key_word: impl Fn(&PerryLayoutKey) -> u64,
) -> bool {
    use std::sync::atomic::{AtomicU64, Ordering};
    if cell.is_null() {
        return false;
    }
    if !crate::value::addr_class::realm::is_image_object(cell as usize) {
        return false;
    }
    if (*cell).length as usize != keys.len() {
        return false;
    }
    #[cfg(all(target_vendor = "apple", target_pointer_width = "64"))]
    if has_attributes(keys) {
        let front = AtomicU64::from_ptr(cell.add(1).cast());
        let raw = front.load(Ordering::Acquire);
        if raw >> 48 == 0 {
            let _ = front.compare_exchange(
                raw,
                raw | crate::value::POINTER_TAG,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
    }
    let values = crate::array::array_elements_ptr(cell);
    for (i, key) in keys.iter().enumerate() {
        let atom = &*key.atom;
        if atom.kind == crate::codegen_abi::ATOM_KIND_BINDING_SYMBOL {
            return false;
        }
        let string = atom.kind == crate::codegen_abi::ATOM_KIND_STRING;
        #[cfg(not(all(target_vendor = "apple", target_pointer_width = "64")))]
        if string {
            continue;
        }
        let expected = if string {
            atom.string as u64
        } else {
            crate::value::TAG_UNDEFINED
        };
        let actual = key_word(key);
        let word = AtomicU64::from_ptr(values.add(i));
        // GC_STORE_AUDIT(INIT): unpublished immutable facts; the string or
        // process-lifetime symbol is immortal and introduces no heap edge.
        let _ = word.compare_exchange(expected, actual, Ordering::AcqRel, Ordering::Acquire);
    }
    true
}
