//! The for-of source walk's counted entry (`perry_hir` `for_of_guard`):
//! `arrayRecordNeedsIterator(t, true)`, lowered by `native_runtime_branch`.

use crate::expr::FnCtx;
use crate::types::{DOUBLE, I32};

/// The for-of walk's counted entry: `(verdict, source)`, where `verdict` is
/// `js_array_record_enter_counted`'s bits (0: the iterator protocol is needed;
/// 1: packed-f64 loop admitted) and `source` the head it resolved.
///
/// When no program write can reach an iteration member or define an element
/// descriptor (`array_program_facts` is 0), the runtime admits an ordinary
/// array exactly on its header, and an array that passes the inline test
/// below is one it would admit unchanged: a `POINTER_TAG` word above the
/// handle band whose head is a non-forwarded `GC_TYPE_ARRAY`, not frozen,
/// sealed or non-extensible, on the default prototype chain, in the raw-f64
/// layout, with `length <= capacity <= PACKED_F64_LOOP_MAX_CAPACITY`. That is
/// `array_record_enter` with `clean_headed_arr_ptr` returning the head itself
/// and `packed_f64_loop_admits_live_array` accepting it. Anything else asks the
/// runtime, which also answers every case the inline test does not decide.
pub(super) fn lower(ctx: &mut FnCtx<'_>, value: &str) -> (String, String) {
    use crate::types::{I1, I16, I64, PTR};
    let writable = crate::expr::array_iteration_facts(&ctx.wpfacts).to_string();
    let call = |ctx: &mut FnCtx<'_>| {
        let slot = ctx.func.alloca_entry(DOUBLE);
        let verdict = ctx.block().call(
            I32,
            "js_array_record_enter_counted",
            &[(DOUBLE, value), (PTR, &slot), (I32, &writable)],
        );
        let source = ctx.block().load(DOUBLE, &slot);
        (verdict, source)
    };
    if crate::expr::array_program_facts(&ctx.wpfacts) != 0 {
        return call(ctx);
    }
    let header_idx = ctx.new_block("forof.walk.header");
    let shape_idx = ctx.new_block("forof.walk.shape");
    let inline_idx = ctx.new_block("forof.walk.inline");
    let runtime_idx = ctx.new_block("forof.walk.runtime");
    let merge_idx = ctx.new_block("forof.walk.merge");
    let header_label = ctx.block_label(header_idx);
    let shape_label = ctx.block_label(shape_idx);
    let inline_label = ctx.block_label(inline_idx);
    let runtime_label = ctx.block_label(runtime_idx);
    let merge_label = ctx.block_label(merge_idx);

    let handle = {
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(value);
        let tag = blk.lshr(I64, &bits, "48");
        let is_pointer = blk.icmp_eq(I64, &tag, POINTER_TAG_HI16);
        let handle = blk.and(I64, &bits, crate::nanbox::POINTER_MASK_I64);
        let above_band = blk.icmp_ugt(
            I64,
            &handle,
            crate::expr::receiver_range::HANDLE_BAND_TOP_LITERAL,
        );
        let candidate = blk.and(I1, &is_pointer, &above_band);
        blk.cond_br(&candidate, &header_label, &runtime_label);
        handle
    };

    ctx.current_block = header_idx;
    {
        let blk = ctx.block();
        let header_addr = blk.sub(I64, &handle, "8");
        let header_ptr = blk.inttoptr(I64, &header_addr);
        let word = blk.load(I32, &header_ptr);
        let masked = blk.and(I32, &word, &WALK_HEADER_MASK.to_string());
        let plain = blk.icmp_eq(I32, &masked, &WALK_HEADER_EXPECT.to_string());
        blk.cond_br(&plain, &shape_label, &runtime_label);
    }

    ctx.current_block = shape_idx;
    {
        let blk = ctx.block();
        let reserved_addr = blk.sub(I64, &handle, "6");
        let reserved_ptr = blk.inttoptr(I64, &reserved_addr);
        let reserved = blk.load(I16, &reserved_ptr);
        let default_chain =
            crate::expr::array_proto_guard::emit_array_default_prototype_chain(blk, &reserved);
        let length = blk.safe_load_i32_from_ptr(&handle);
        let capacity_addr = blk.add(I64, &handle, "4");
        let capacity_ptr = blk.inttoptr(I64, &capacity_addr);
        let capacity = blk.load(I32, &capacity_ptr);
        let fits = blk.icmp_ule(I32, &length, &capacity);
        let bounded = blk.icmp_ule(
            I32,
            &capacity,
            &perry_abi::PACKED_F64_LOOP_MAX_CAPACITY.to_string(),
        );
        let sized = blk.and(I1, &fits, &bounded);
        let admitted = blk.and(I1, &default_chain, &sized);
        blk.cond_br(&admitted, &inline_label, &runtime_label);
    }

    ctx.current_block = inline_idx;
    let inline_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = runtime_idx;
    let (runtime_verdict, runtime_source) = call(ctx);
    let runtime_end = ctx.block().label.clone();
    ctx.block().br(&merge_label);

    ctx.current_block = merge_idx;
    let blk = ctx.block();
    let verdict = blk.phi(I32, &[("2", &inline_end), (&runtime_verdict, &runtime_end)]);
    let source = blk.phi(
        DOUBLE,
        &[(value, &inline_end), (&runtime_source, &runtime_end)],
    );
    (verdict, source)
}

/// `0x7FFD`: the NaN-box tag of a heap pointer.
const POINTER_TAG_HI16: &str = "32765";

/// The walk entry's mask over the little-endian i32 at `head - 8`
/// (`GcHeader { obj_type: u8, gc_flags: u8, _reserved: u16 }`): the type
/// byte, `GC_FLAG_FORWARDED`, and the `_reserved` bits frozen, sealed,
/// non-extensible and raw-f64 layout.
const WALK_HEADER_MASK: u32 = 0xFF
    | (perry_abi::GC_FLAG_FORWARDED as u32) << 8
    | ((perry_abi::OBJ_FLAG_FROZEN
        | perry_abi::OBJ_FLAG_SEALED
        | perry_abi::OBJ_FLAG_NO_EXTEND
        | perry_abi::GC_ARRAY_RAW_F64_LAYOUT) as u32)
        << 16;
/// What [`WALK_HEADER_MASK`] must leave: a non-forwarded `GC_TYPE_ARRAY`,
/// no integrity flag, raw-f64 layout.
const WALK_HEADER_EXPECT: u32 =
    perry_abi::GC_TYPE_ARRAY as u32 | (perry_abi::GC_ARRAY_RAW_F64_LAYOUT as u32) << 16;
