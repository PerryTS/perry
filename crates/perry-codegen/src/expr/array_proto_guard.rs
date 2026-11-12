//! #10593: the inline array-index guards' "no inherited index can intercept
//! this access" test.
//!
//! An element access may take a raw-slot fast path only while no prototype
//! on the receiver's chain can carry an indexed property. Two facts decide
//! that, and every guard needs both:
//!
//! * the process-wide byte `PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED`: an index
//!   installed on `Array.prototype` / `Object.prototype`, or a retargeted
//!   `Array.prototype` — changes to the DEFAULT chain every array shares;
//! * the receiver's own `GC_ARRAY_CUSTOM_PROTO` bit (perry-runtime
//!   `gc/types.rs`): `Object.setPrototypeOf(thisArray, p)`.
//!
//! The second used to be folded into the first, so one retargeted array
//! anywhere stood every array's fast path down for the rest of the process —
//! including the element store's cheap barrier, which made every element store
//! in the program take the full old-to-young write barrier (33x whole-program
//! on the issue's fixture).

use crate::block::LlBlock;
use crate::types::{I1, I16, I8};

/// `GC_ARRAY_CUSTOM_PROTO` (bit 6 of a `GC_TYPE_ARRAY` header's `_reserved`
/// word). MUST match perry-runtime `gc/types.rs`.
pub(crate) const GC_ARRAY_CUSTOM_PROTO_I16: &str = "64";

/// Emit the i1 "the receiver's prototype chain is the pristine default one"
/// test for a `GC_TYPE_ARRAY` receiver whose `_reserved` word is `reserved`
/// (an `i16` already loaded by the caller's header guard).
pub(crate) fn emit_array_default_prototype_chain(blk: &mut LlBlock, reserved: &str) -> String {
    let invalidated = blk.load_volatile(I8, "@PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED");
    let index_bit = blk.and(I8, &invalidated, INDEX_INVALIDATED_I8);
    let globally_clean = blk.icmp_eq(I8, &index_bit, "0");
    let custom_proto_bit = blk.and(I16, reserved, GC_ARRAY_CUSTOM_PROTO_I16);
    let own_proto_default = blk.icmp_eq(I16, &custom_proto_bit, "0");
    blk.and(I1, &globally_clean, &own_proto_default)
}

/// `PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED`'s index bit (runtime
/// `array::INDEX_INVALIDATED`): an index on the default chain. The other bit
/// (`ARRAY_METHODS_MODIFIED`, #11915) only concerns the method admission
/// below, so an index guard masks it out.
pub(crate) const INDEX_INVALIDATED_I8: &str = "1";

/// [`emit_array_default_prototype_chain`] for an inline array METHOD body
/// (`push`/`pop`): the receiver's chain is the default one AND
/// `%Array.prototype%` / `%Object.prototype%` are unmodified (#11915,
/// `builtin_proto`), so the method the inline body implements is the one the
/// receiver would read. The summary byte the admission already loads carries
/// both facts (`ARRAY_METHODS_MODIFIED` mirrors the Array byte), so the whole
/// byte is tested: no new load, no new branch, no new bytes.
pub(crate) fn emit_array_method_admission(blk: &mut LlBlock, reserved: &str) -> String {
    let invalidated = blk.load_volatile(I8, "@PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED");
    let globally_clean = blk.icmp_eq(I8, &invalidated, "0");
    let custom_proto_bit = blk.and(I16, reserved, GC_ARRAY_CUSTOM_PROTO_I16);
    let own_proto_default = blk.icmp_eq(I16, &custom_proto_bit, "0");
    blk.and(I1, &globally_clean, &own_proto_default)
}

/// `ArrayHeader.named`'s byte offset (perry-runtime `array/header.rs`, object
/// model §6a): null until the array owns a property beyond its elements.
pub(crate) const ARRAY_NAMED_OFFSET: &str = "8";

/// Emit the i1 "the array owns a property beyond its elements" test for a
/// live `GC_TYPE_ARRAY` head whose address is the i64 `arr_addr`. Every such
/// property (a named or symbol key, an index-key descriptor, a non-writable
/// `length`) lives in the array's named layout object; an inline element or
/// length fast path admits a null `named` and leaves anything else to the
/// runtime, which reads the layout's element-kind fact exactly
/// (`array_has_element_descriptors`).
pub(crate) fn emit_array_has_named_layout(blk: &mut LlBlock, arr_addr: &str) -> String {
    let named_addr = blk.add(crate::types::I64, arr_addr, ARRAY_NAMED_OFFSET);
    let named_ptr = blk.inttoptr(crate::types::I64, &named_addr);
    let named = blk.load(crate::types::I64, &named_ptr);
    blk.icmp_ne(crate::types::I64, &named, "0")
}

/// Branch to `plain_label` when the NaN-boxed `recv` is a live, unforwarded
/// `GC_TYPE_ARRAY` head that owns no named property and reads its methods from
/// the unmodified `%Array.prototype%` (so `recv.<method>` IS the realm's
/// builtin), else to `other_label`, where the caller performs the real `Get`.
/// The receiver is only annotated an Array, so the header is read behind the
/// heap-pointer band test (the test `class_field_push` makes).
pub(crate) fn emit_plain_array_method_branch(
    ctx: &mut super::FnCtx<'_>,
    recv: &str,
    plain_label: &str,
    other_label: &str,
) {
    use crate::types::I64;
    let hdr = ctx.new_block("arrmethod.header");
    let hdr_label = ctx.block_label(hdr);
    let handle = {
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(recv);
        let tag = blk.lshr(I64, &bits, "48");
        let pointer = blk.icmp_eq(I64, &tag, "32765");
        let handle = blk.and(I64, &bits, "281474976710655");
        let above = blk.icmp_ugt(I64, &handle, super::receiver_range::HANDLE_BAND_TOP_LITERAL);
        let below = blk.icmp_ult(I64, &handle, "140737488355328");
        let range = blk.and(I1, &above, &below);
        let heap = blk.and(I1, &pointer, &range);
        blk.cond_br(&heap, &hdr_label, other_label);
        handle
    };
    ctx.current_block = hdr;
    let blk = ctx.block();
    let ptr = blk.inttoptr(I64, &handle);
    let kind_ptr = blk.gep(I8, &ptr, &[(I64, "-8")]);
    let kind = blk.load(I8, &kind_ptr);
    let array = crate::target_layout::emit_gc_type_is_array(blk, I8, &kind);
    let flags_ptr = blk.gep(I8, &ptr, &[(I64, "-7")]);
    let flags = blk.load(I8, &flags_ptr);
    let forwarded = blk.and(I8, &flags, "128");
    let live = blk.icmp_eq(I8, &forwarded, "0");
    let reserved_ptr = blk.gep(I8, &ptr, &[(I64, "-6")]);
    let reserved = blk.load(I16, &reserved_ptr);
    let has_named = emit_array_has_named_layout(blk, &handle);
    let no_own = blk.xor(I1, &has_named, "true");
    let default_proto = emit_array_method_admission(blk, &reserved);
    let plain = blk.and(I1, &array, &live);
    let method = blk.and(I1, &no_own, &default_proto);
    let ok = blk.and(I1, &plain, &method);
    blk.cond_br(&ok, plain_label, other_label);
}

/// Does any code of the program define an element descriptor on an array
/// (`patched_builtins::ARRAY_ELEMENT_DESCRIPTORS`)? Without one no array has
/// element descriptors and the per-read query is not emitted at all.
pub(crate) fn program_writes_element_descriptors() -> bool {
    crate::program_patched_proto_methods()
        .iter()
        .any(|name| name == perry_hir::patched_builtins::ARRAY_ELEMENT_DESCRIPTORS)
}

/// Closed World excludes indexed descriptors in programs which never obtain
/// a descriptor writer. Otherwise query the receiver's layout element kind.
pub(crate) fn emit_array_element_descriptors(blk: &mut LlBlock, arr_addr: &str) -> String {
    if !program_writes_element_descriptors() {
        return "false".to_owned();
    }
    let kind = blk.call(
        crate::types::I32,
        "perry_array_element_kind",
        &[(crate::types::I64, arr_addr)],
    );
    blk.icmp_ne(crate::types::I32, &kind, "0")
}

/// Emit the i1 "an ordinary, unforwarded `GC_TYPE_ARRAY` with no element
/// descriptors" test for the live heap handle `handle` (an i64 address the
/// caller has already placed in the heap-pointer band): the receiver guard of
/// every inline element read. A call-free region reads the dense slots of an
/// array admitted by it.
pub(crate) fn emit_ordinary_array_admission(blk: &mut LlBlock, handle: &str) -> String {
    let word = super::index_get::guarded_array::emit_array_guard_word(blk, handle);
    super::index_get::guarded_array::emit_array_guard_word_ok(blk, &word, handle)
}

/// `bits - POINTER_TAG - 2 MiB`, compared unsigned against this span, holds
/// exactly for a heap-pointer-tagged value above the runtime-id band: the
/// precondition of reading the header behind it.
pub(crate) const HEAP_POINTER_BAND_BASE_I64: &str = super::receiver_range::RECEIVER_BIAS_LITERAL;
/// The span of the band that [`HEAP_POINTER_BAND_BASE_I64`] biases into.
pub(crate) const HEAP_POINTER_BAND_SPAN_I64: &str = super::receiver_range::RECEIVER_SPAN_LITERAL;
