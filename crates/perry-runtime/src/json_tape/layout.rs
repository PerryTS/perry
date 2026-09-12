//! Layout contracts on [`LazyArrayHeader`] that emitted code depends on.
//!
//! Perry's codegen reads `.length` as a raw u32 at offset 0 instead of calling
//! into the runtime. A field reordered in front of it would send emitted code
//! at an unrelated word with every test still green, so the offset is pinned
//! here at compile time. The doc comment on the field itself says *why* it is
//! load-bearing; this module is only the enforcement.
//!
//! The indexed inline cache reaches the other words through
//! `js_lazy_array_index_probe` rather than emitting their offsets, so they need
//! no pin: moving them is a plain Rust refactor the compiler checks.

use super::LazyArrayHeader;

// `cached_length` at offset 0 is a CODEGEN contract, not a layout preference:
// Perry inlines `.length` as a raw u32 load at offset 0 rather than calling
// `js_array_length`, so an unmaterialized lazy array only reports the right
// length because this field sits first. Nothing else in the tree enforced
// that — the guarantee lived in a doc comment — so a field reordered into
// the front would have produced silently wrong `.length` values with every
// test still green. Adding a field to this struct is the moment that can
// happen, so pin it here.
const _: () = assert!(
    std::mem::offset_of!(LazyArrayHeader, cached_length) == 0,
    "LazyArrayHeader::cached_length must stay at offset 0 — codegen inlines \
     `.length` as a raw u32 load there"
);
