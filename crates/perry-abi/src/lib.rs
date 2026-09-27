#![no_std]
//! Runtime layout facts that generated code bakes in, in ONE file both sides
//! depend on: `perry-runtime` (as `crate::codegen_abi`) and `perry-codegen`
//! (as `crate::runtime_abi`). A layout change edits a number here, and the
//! runtime's `offset_of!`/`size_of` assertions next to each struct refuse to
//! compile until the number is right — so emitted code can never disagree
//! with the struct it indexes. No dependencies.

/// `array::ArrayHeader` size: element 0 follows it.
pub const ARRAY_HEADER_SIZE: usize = 8;

/// `agent_ptrs::PERRY_AGENT_PTRS`: the number of per-agent pointer slots.
/// Slot 0 is reserved (the megamorphic follow-up's shape-record directory).
pub const AGENT_PTR_SLOTS: usize = 4;
/// Slot 1: the address of this agent's implicit-`this` cell
/// (`tls_hot::HotTls::implicit_this`), which a direct method call binds.
pub const AGENT_PTR_IMPLICIT_THIS: usize = 1;
/// `tls_hot::HotTls::agent_ptrs` (Apple aarch64 TSD path; LP64): directly
/// after `implicit_this` (128), behind fixed-size fields only.
pub const HOT_TLS_AGENT_PTRS_OFFSET: usize = 136;

/// `closure::ClosureHeader` (LP64): the code pointer at 0, the u32 capture
/// count at 8, the u32 type tag at 12. A value is a closure when its type tag
/// is [`CLOSURE_MAGIC`] (`closure::is_closure_ptr`'s selective term).
pub const CLOSURE_FUNC_PTR_OFFSET: usize = 0;
pub const CLOSURE_TYPE_TAG_OFFSET: usize = 12;
/// `closure::CLOSURE_MAGIC` ("CLOS").
pub const CLOSURE_MAGIC: u32 = 0x434C_4F53;

/// `object::method_site::MethodEntry` — the words the emitted method-call site
/// reads (`perry-codegen/src/expr/method_site.rs`).
pub const METHOD_SITE_WORD_OFFSET: usize = 0;
pub const METHOD_SITE_SLOT_OFFSET: usize = 8;
pub const METHOD_SITE_FUNC_OFFSET: usize = 16;
pub const METHOD_SITE_CLOSURE_OFFSET: usize = 24;
pub const METHOD_SITE_GEN_OFFSET: usize = 32;
/// Entries per method site, and one entry's size.
pub const METHOD_SITE_WAYS: usize = 2;
pub const METHOD_SITE_ENTRY_SIZE: usize = 40;
/// A method site calls a body with its argument count padded by `undefined`
/// up to this many extra arguments (never past 16), and admits bodies that
/// declare up to that many parameters.
pub const METHOD_SITE_ARG_PAD: usize = 3;
pub const fn method_site_padded_argc(argc: usize) -> usize {
    let padded = argc + METHOD_SITE_ARG_PAD;
    if padded > 16 {
        if argc > 16 {
            argc
        } else {
            16
        }
    } else {
        padded
    }
}
/// The entry `slot` bit for an own key in the receiver's spill buffer.
pub const METHOD_SITE_SPILL: u64 = 1 << 62;
/// The index bits of an entry's `slot` word (bits 61 and 60 are reserved for
/// the function-bag and accessor entry kinds).
pub const METHOD_SITE_INDEX_MASK: u64 = (1 << 60) - 1;
/// `object::ObjectMeta::spill` (the object-owned overflow buffer).
pub const OBJECT_META_SPILL_OFFSET: usize = 32;
