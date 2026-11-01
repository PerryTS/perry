//! A literal site owns immutable matcher data, never a RegExp instance.
//! Workers must not read or write the primary agent's image-global word.
//! The word uses the existing mutable global-root contract, so both its
//! RegExpData edge and the data's string/program edges survive evacuation.
use super::{RegExpData, RegExpHeader};
use crate::gc::RuntimeHandleScope;
use crate::string::{StaticDispatchString, StringHeader};

/// Immortal codegen descriptor. The source/flags addresses name the module's
/// immutable static string descriptors (read-only data, valid in every agent
/// and on every object format; a string handle global may be thread-local,
/// and a thread-local's address cannot sit in static data). Only `data` is
/// mutable, through the global-root store contract. No source/flags loads
/// belong to a repeated evaluation.
#[repr(C)]
pub(crate) struct LiteralSite {
    pub(crate) pattern: *const StaticDispatchString,
    pub(crate) flags: *const StaticDispatchString,
    pub(crate) data: u64,
}

#[no_mangle]
pub extern "C" fn js_regexp_literal(site: i64) -> *mut RegExpHeader {
    let site = site as *mut LiteralSite;
    let word = unsafe { std::ptr::addr_of_mut!((*site).data) };
    if crate::agent::current_agent() == crate::agent::PRIMARY_AGENT && unsafe { word.read() } != 0 {
        return super::instance::new(|| unsafe {
            (word.read() & crate::value::POINTER_MASK) as *const RegExpData
        });
    }
    literal_cold(site, word)
}

#[cold]
#[optimize(size)]
#[inline(never)]
fn literal_cold(site: *mut LiteralSite, word: *mut u64) -> *mut RegExpHeader {
    let scope = RuntimeHandleScope::new();
    let (pattern, flags) = unsafe {
        let born = |text: *const StaticDispatchString| {
            crate::string::js_string_from_wtf8_bytes((*text).bytes, (*text).byte_len)
        };
        let pattern = scope.root_string_ptr(born((*site).pattern));
        (pattern, scope.root_string_ptr(born((*site).flags)))
    };
    pattern.with_const_ptr(|pattern| {
        flags.with_const_ptr(|flags| from_parts(pattern, flags, word as i64))
    })
}

// The cold miss and runtime fixtures share exactly the same publication and
// birth. Runtime fixtures supply raw strings and an immortal mutable word.
pub(crate) fn from_parts(
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
#[optimize(size)]
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
