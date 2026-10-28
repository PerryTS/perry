//! Fixed births reuse the current nursery cursor. Refusal (including an
//! incremental mark's live birth color) takes the existing collecting helper.
//! No managed pointer crosses that helper except through its original ABI.
use crate::expr::FnCtx;
use crate::types::{I1, I32, I64, I8, PTR};

/// Keep the existing allocator on ILP32 and for cells outside the nursery's
/// small-object contract. The emitted fast path uses the LP64 header ABI.
pub(super) fn eligible(target: &str, size: u64) -> bool {
    let arch = target.split('-').next().unwrap_or_default();
    // Packed header words use the shipped little-endian LP64 ABIs. Other
    // targets keep the runtime's field-wise initialization.
    matches!(
        arch,
        "x86_64" | "x86_64h" | "amd64" | "aarch64" | "arm64" | "arm64e"
    ) && !crate::target_layout::target_is_ilp32(target)
        && size <= 16 * 1024
}

/// The birth's handle, and with `written` an `i1` that is true on the
/// emitted arm (which ran `initialize`) and false on the runtime arm.
fn allocate(
    ctx: &mut FnCtx<'_>,
    size: u64,
    admitted: Option<&str>,
    written: bool,
    initialize: impl FnOnce(&mut FnCtx<'_>, &str) -> String,
    slow: impl FnOnce(&mut FnCtx<'_>) -> String,
) -> (String, Option<String>) {
    let state = crate::expr::load_inline_arena_state(ctx);
    let blk = ctx.block();
    let offset_ptr = blk.gep(I8, &state, &[(I64, "8")]);
    let offset = blk.load(I64, &offset_ptr);
    let next = blk.add(I64, &offset, &size.to_string());
    let limit_ptr = blk.gep(I8, &state, &[(I64, "16")]);
    let limit = blk.load(I64, &limit_ptr);
    // The runtime's canonical bump bound is zero during allocate-black.
    // Only descriptor admission remains a separate condition.
    let limit = if let Some(admitted) = admitted {
        blk.select(I1, admitted, I64, &limit, "0")
    } else {
        limit
    };
    let fits = blk.icmp_ule(I64, &next, &limit);
    let fast = ctx.new_block("fixed_birth.fast");
    let refused = ctx.new_block("fixed_birth.runtime");
    let merge = ctx.new_block("fixed_birth.ready");
    let fast_label = ctx.block_label(fast);
    let refused_label = ctx.block_label(refused);
    let merge_label = ctx.block_label(merge);
    ctx.block().cond_br(&fits, &fast_label, &refused_label);
    ctx.current_block = fast;
    let blk = ctx.block();
    // GC_STORE_AUDIT(INIT): the existing nursery cursor, no new allocator state.
    blk.store(I64, &next, &offset_ptr);
    let data = blk.load(PTR, &state);
    let raw = blk.gep(I8, &data, &[(I64, &offset)]);
    let fast_value = initialize(ctx, &raw);
    let fast_pred = ctx.block().label.clone();
    ctx.block().br(&merge_label);
    ctx.current_block = refused;
    let slow_value = slow(ctx);
    let slow_pred = ctx.block().label.clone();
    ctx.block().br(&merge_label);
    ctx.current_block = merge;
    let written = written.then(|| {
        ctx.block()
            .phi(I1, &[("true", &fast_pred), ("false", &slow_pred)])
    });
    let value = ctx
        .block()
        .phi(I64, &[(&fast_value, &fast_pred), (&slow_value, &slow_pred)]);
    (value, written)
}

/// The original runtime birth remains the slow arm, with its unchanged rooted
/// input contract. UNKNOWN scans tagged and raw box captures; empty closures
/// retain the runtime's POINTER_FREE state. Every slot is written before any
/// call or publication, so an idle nursery birth needs no insertion barrier.
pub(crate) fn closure(
    ctx: &mut FnCtx<'_>,
    info: &str,
    header: &str,
    words: &[String],
    slow: impl FnOnce(&mut FnCtx<'_>) -> String,
) -> String {
    let size = 8 + crate::runtime_abi::CLOSURE_HEADER_SIZE as u64 + words.len() as u64 * 8;
    if !eligible(ctx.target_triple, size) {
        return slow(ctx);
    }
    allocate(
        ctx,
        size,
        None,
        false,
        |ctx, raw| {
            let reserved = if words.is_empty() { 0x4000u64 } else { 0 };
            let packed = u64::from(crate::runtime_abi::GC_TYPE_CLOSURE)
                | (0x02 << 8)
                | (reserved << 16)
                | (size << 32);
            let blk = ctx.block();
            // GC_STORE_AUDIT(INIT): identical idle GcHeader and closure header image.
            blk.store(I64, &packed.to_string(), raw);
            let user = blk.gep(I8, raw, &[(I64, "8")]);
            blk.store(I64, header, &user);
            let body = blk.gep(
                I8,
                &user,
                &[(I64, &crate::runtime_abi::CLOSURE_INFO_OFFSET.to_string())],
            );
            blk.store(PTR, info, &body);
            let props = blk.gep(
                I8,
                &user,
                &[(I64, &crate::runtime_abi::CLOSURE_PROPS_OFFSET.to_string())],
            );
            // GC_STORE_AUDIT(INIT): fresh closure's null own-property edge.
            blk.store(PTR, "null", &props);
            for (slot, word) in words.iter().enumerate() {
                let address = blk.gep(
                    I8,
                    &user,
                    &[(
                        I64,
                        &(crate::runtime_abi::CLOSURE_HEADER_SIZE + slot * 8).to_string(),
                    )],
                );
                // GC_STORE_AUDIT(INIT): complete capture range of an idle nursery birth.
                blk.store(I64, word, &address);
            }
            blk.ptrtoint(&user, I64)
        },
        slow,
    )
    .0
}

/// Only compiler-emitted ordinary layouts enter here: zero integrity, default
/// prototype, and key attributes either default or OPTIONAL. Their birth
/// record is exactly PLAIN_ORDINARY; slots (including optional keys) are holes.
/// The descriptor, registered before generated code runs, supplies the ShapeId.
pub(crate) fn layout(ctx: &mut FnCtx<'_>, desc: &str, slots: u32, class: u32) -> String {
    layout_with(ctx, desc, slots, class, None).0
}

/// [`layout`], with a complete constructor prologue's values (`(slot, bits)`)
/// written in the emitted arm instead of their holes, as the class birth's
/// compact form does (`fixed_class`): the idle nursery birth is young and
/// unpublished, so they need no barrier. The `i1` is true on that arm; the
/// runtime arm's object holds holes and runs the constructor.
pub(crate) fn layout_with(
    ctx: &mut FnCtx<'_>,
    desc: &str,
    slots: u32,
    class: u32,
    fields: Option<&[(u32, String)]>,
) -> (String, Option<String>) {
    let size = 8
        + crate::target_layout::object_header_size_bytes(ctx.target_triple)
        + u64::from(slots) * 8;
    let slow = |ctx: &mut FnCtx<'_>| {
        let raw = ctx.block().call(PTR, "perry_alloc_layout", &[(PTR, desc)]);
        ctx.block().ptrtoint(&raw, I64)
    };
    if !eligible(ctx.target_triple, size) {
        return (slow(ctx), None);
    }
    let value = |slot: u32| {
        fields
            .and_then(|f| f.iter().find(|(s, _)| *s == slot))
            .map_or_else(|| crate::nanbox::TAG_HOLE.to_string(), |(_, v)| v.clone())
    };
    // Whole-image templates are immutable and the image plan is exactly
    // what emitted desc.layout_id. Consume that proof instead of reloading
    // and testing old registration sentinels for each object. An opaque or
    // ambiguous descriptor keeps the original checked admission.
    let template = desc
        .strip_prefix('@')
        .and_then(|symbol| crate::layout_ordinals::current_plan().descriptor_template_word(symbol));
    let (id, registered) = if let Some(word) = template {
        ((word as i32).to_string(), None)
    } else {
        let id_ptr = ctx.block().gep(
            I8,
            desc,
            &[(I64, &crate::layout_desc::DESC_LAYOUT_ID_OFFSET.to_string())],
        );
        let id = ctx.block().load(I32, &id_ptr);
        let nonzero = ctx.block().icmp_ne(I32, &id, "0");
        let registered = ctx.block().icmp_ne(I32, &id, "-1");
        let registered = ctx.block().and(I1, &nonzero, &registered);
        (id, Some(registered))
    };
    allocate(
        ctx,
        size,
        registered.as_deref(),
        fields.is_some(),
        |ctx, raw| {
            let blk = ctx.block();
            // GC_STORE_AUDIT(INIT): runtime mark_layout_object_facts for an ordinary
            // compiler descriptor: PLAIN_ORDINARY, no integrity or descriptor bits.
            let packed = 2u64 | (0x02 << 8) | (0x200 << 16) | (size << 32);
            blk.store(I64, &packed.to_string(), raw);
            let user = blk.gep(I8, raw, &[(I64, "8")]);
            blk.store(I32, &class.to_string(), &user);
            let shape_ptr = blk.gep(I8, &user, &[(I64, "4")]);
            blk.store(I32, &id, &shape_ptr);
            let meta = blk.gep(I8, &user, &[(I64, "8")]);
            // GC_STORE_AUDIT(INIT): unpublished layout object, no per-object meta.
            blk.store(PTR, "null", &meta);
            if slots <= 8 || fields.is_some() {
                for slot in 0..slots {
                    let address = blk.gep(I8, &user, &[(I64, &(16 + slot * 8).to_string())]);
                    // GC_STORE_AUDIT(INIT): every declared slot begins absent, or
                    // holds its prologue value in an idle young birth.
                    blk.store(I64, &value(slot), &address);
                }
            } else {
                // A bounded loop avoids replicating a large layout's fill at each site.
                let before = blk.label.clone();
                let fill = ctx.new_block("fixed_birth.holes");
                let done = ctx.new_block("fixed_birth.filled");
                let fill_label = ctx.block_label(fill);
                let done_label = ctx.block_label(done);
                let next = ctx.block().next_reg();
                ctx.block().br(&fill_label);
                ctx.current_block = fill;
                let blk = ctx.block();
                let slot = blk.phi(I64, &[("0", &before), (&next, &fill_label)]);
                let fields = blk.gep(I8, &user, &[(I64, "16")]);
                let address = blk.gep(I64, &fields, &[(I64, &slot)]);
                // GC_STORE_AUDIT(INIT): complete hole fill, no safepoint in this loop.
                blk.store(I64, &crate::nanbox::TAG_HOLE.to_string(), &address);
                blk.emit_raw(format!("{next} = add i64 {slot}, 1"));
                let more = blk.icmp_ult(I64, &next, &slots.to_string());
                blk.cond_br(&more, &fill_label, &done_label);
                ctx.current_block = done;
            }
            ctx.block().ptrtoint(&user, I64)
        },
        slow,
    )
}
