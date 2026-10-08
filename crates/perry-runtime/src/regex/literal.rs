//! A literal site owns immutable matcher data, never a RegExp instance.
//! Workers must not read or write the primary agent's image-global word.
//! The word uses the existing mutable global-root contract, so both its
//! RegExpData edge and the data's string/program edges survive evacuation.
use super::{RegExpData, RegExpHeader};
use crate::gc::RuntimeHandleScope;
use crate::string::StringHeader;

/// `site_word` is an immortal, aligned, zero-initialized codegen global unique
/// to this literal. Only the primary JS agent accesses it. On first use it
/// becomes an existing global root; subsequent evaluations allocate a fresh
/// ordinary object with its own lastIndex around the same immutable data.
#[no_mangle]
pub extern "C" fn js_regexp_literal(
    pattern: *const StringHeader,
    flags: *const StringHeader,
    site_word: i64,
) -> *mut RegExpHeader {
    if crate::agent::current_agent() != crate::agent::PRIMARY_AGENT {
        return super::perex_api::finish(super::perex_construct::new(pattern, flags));
    }
    let word = site_word as *mut u64;
    if unsafe { word.read() } == 0 {
        literal_miss(pattern, flags, word);
    }
    // The word is a registered global root: after the birth's allocation it
    // holds the data cell's current address.
    super::instance::new(|| unsafe {
        (word.read() & crate::value::POINTER_MASK) as *const RegExpData
    })
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
