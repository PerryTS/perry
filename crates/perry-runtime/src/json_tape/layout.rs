//! Layout contracts on [`LazyArrayHeader`] that emitted code and the in-place
//! materialization depend on.
//!
//! A lazy array is born with an `ArrayHeader` prefix and its element slots,
//! and `flip_to_array` turns it into that array by changing the GC header's
//! `obj_type` alone. Every prefix field therefore has to sit exactly where the
//! `ArrayHeader` keeps it, and the lazy fields must fill whole element words
//! so the slots start where `array_elements_ptr` (end of allocation minus
//! `capacity`) finds them. A field reordered or resized here would send
//! emitted code, or the materialized array, at an unrelated word with every
//! test still green, so the offsets are pinned at compile time.
//!
//! `cached_length` at offset 0 is also a CODEGEN contract on the lazy value
//! itself: Perry inlines `.length` as a raw u32 load at offset 0 rather than
//! calling `js_array_length`.

use super::LazyArrayHeader;
use crate::array::ArrayHeader;
use std::mem::{offset_of, size_of};

const _: () = {
    assert!(offset_of!(LazyArrayHeader, cached_length) == 0);
    assert!(offset_of!(LazyArrayHeader, cached_length) == offset_of!(ArrayHeader, length));
    assert!(offset_of!(LazyArrayHeader, capacity) == offset_of!(ArrayHeader, capacity));
    assert!(offset_of!(LazyArrayHeader, named) == offset_of!(ArrayHeader, named));
    assert!(offset_of!(LazyArrayHeader, ek_prefix) == offset_of!(ArrayHeader, ek_prefix));
    assert!(offset_of!(LazyArrayHeader, ek_kind) == offset_of!(ArrayHeader, ek_kind));
    assert!(offset_of!(LazyArrayHeader, magic) == size_of::<ArrayHeader>());
    // The lazy fields are whole element words of front reserve.
    assert!(size_of::<LazyArrayHeader>() % 8 == 0);
};
