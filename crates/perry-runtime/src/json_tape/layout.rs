//! Layout contracts on [`LazyArrayHeader`] that emitted code depends on.
//!
//! Perry's codegen reads these words directly — `.length` as a raw u32 at
//! offset 0, and the indexed inline cache's lazy tiers at the three pointer
//! slots below — instead of calling into the runtime. A field reordered in
//! front of any of them would send emitted code at an unrelated word with
//! every test still green, so each offset is pinned here at compile time.
//! The doc comments on the fields themselves say *why* each is load-bearing;
//! this module is only the enforcement.

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

// `materialized` is the second codegen contract on this struct. The indexed
// inline cache (`perry-codegen` `expr/index_get/inline_dyn_typed_array.rs`)
// reads this slot directly to serve `lazy[i]` without a runtime call, exactly
// as `cached_read::lazy_get` does. A reordered field would send that fast path
// at an unrelated word, so pin the offset the same way `cached_length` is.
const _: () = assert!(
    std::mem::offset_of!(LazyArrayHeader, materialized) == 32,
    "LazyArrayHeader::materialized must stay at offset 32 — the indexed inline \
     cache loads the installed array from that word"
);

// The sparse tier of that same cache probes the per-element cache directly:
// bitmap bit first, then the parallel element slot. Both offsets are read as
// raw words from emitted code, so neither may drift either.
const _: () = assert!(
    std::mem::offset_of!(LazyArrayHeader, materialized_elements) == 40,
    "LazyArrayHeader::materialized_elements must stay at offset 40 — the \
     indexed inline cache loads a cached element from that word"
);
const _: () = assert!(
    std::mem::offset_of!(LazyArrayHeader, materialized_bitmap) == 48,
    "LazyArrayHeader::materialized_bitmap must stay at offset 48 — the indexed \
     inline cache proves a cached element live from that word"
);
