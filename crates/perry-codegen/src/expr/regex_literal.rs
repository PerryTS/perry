//! A RegExp literal's evaluation: a fresh ordinary RegExp around the site's
//! immutable matcher data (`perry-runtime/src/regex/literal.rs`).
//!
//! The site is two words. Word 0 holds the matcher data, a registered global
//! root the runtime publishes on the site's first evaluation. Word 1 holds
//! the instance's object-header word (`class_id | birth ShapeId << 32`); the
//! runtime publishes it after word 0, only when the birth shape's two lanes
//! are `Any`, and marks that shape externally carried, so it is never pruned
//! while the site names it. Emitted:
//!
//! ```text
//!   workers == 0 && word1 != 0                      else SLOW
//!   raw = bump 40 bytes from the inline arena        else SLOW
//!   [raw]      = <GC header | birth flags, word1>    (one vector store)
//!   [raw + 16] = meta 0
//!   [raw + 24] = word0 (data)  ; [raw + 32] = +0 (lastIndex)
//!   seed the birth (inline_birth::finish): AFTER every slot holds its value
//!   SLOW: js_regexp_literal(pattern, flags, site)    (first evaluation,
//!         a worker agent, no arena room: the runtime birth)
//! ```
//!
//! Nothing between reading word 0 and the slot store can collect, so the
//! data address stored is current. The newborn is in the nursery, so the
//! store needs no remembered-set entry; during incremental marking the birth
//! is seeded only once both slots are initialized (the birth protocol the
//! inline `new` uses).
use super::FnCtx;
use crate::types::{DOUBLE, I1, I64, I8, PTR};

/// GcHeader (8) + ObjectHeader (16) + two slots: matcher data, lastIndex.
const REGEXP_FIELD_COUNT: u32 = 2;
/// `perry-runtime` `gc::OBJ_FLAG_PLAIN_ORDINARY`, in the header's `_reserved`
/// half-word (bits 16..31): runtime births premark it too.
const PLAIN_ORDINARY_RESERVED_BITS: u64 = 0x200 << 16;

/// The site's two words, `[2 x i64]`, unique per literal and zero until the
/// runtime publishes them.
pub(crate) fn emit_regexp_site(ctx: &mut FnCtx<'_>) -> String {
    let site_id = ctx.ic_site_counter;
    ctx.ic_site_counter += 1;
    let prefix = ctx.strings.module_prefix();
    let slot_name = if prefix.is_empty() {
        format!("perry_regexp_site_{site_id}")
    } else {
        format!("perry_regexp_site_{prefix}__{site_id}")
    };
    ctx.typed_parse_rodata.push(format!(
        "@{slot_name} = private global [2 x i64] zeroinitializer, align 8"
    ));
    format!("@{slot_name}")
}

/// Whether this target uses the 64-bit inline-allocation ABI the fast arm
/// assumes (8-byte slots behind a 16-byte object header).
fn inline_birth_supported(triple: &str) -> bool {
    (triple.starts_with("x86_64") || triple.starts_with("aarch64") || triple.starts_with("arm64"))
        && !triple.contains("32")
        && !triple.contains("wasm")
}

/// Lower `/pattern/flags` to an i64 object handle.
pub(crate) fn lower_regexp_literal(ctx: &mut FnCtx<'_>, pattern: &str, flags: &str) -> String {
    let pattern_idx = ctx.strings.intern(pattern);
    let flags_idx = ctx.strings.intern(flags);
    let pattern_global = format!("@{}", ctx.strings.entry(pattern_idx).handle_global);
    let flags_global = format!("@{}", ctx.strings.entry(flags_idx).handle_global);
    let site = emit_regexp_site(ctx);
    let slow_call = |ctx: &mut FnCtx<'_>| {
        let blk = ctx.block();
        let pattern_box = blk.load(DOUBLE, &pattern_global);
        let flags_box = blk.load(DOUBLE, &flags_global);
        let pattern_handle = super::unbox_to_i64(blk, &pattern_box);
        let flags_handle = super::unbox_to_i64(blk, &flags_box);
        let site_key = blk.ptrtoint(&site, I64);
        blk.call(
            I64,
            "js_regexp_literal",
            &[
                (I64, &pattern_handle),
                (I64, &flags_handle),
                (I64, &site_key),
            ],
        )
    };
    if !inline_birth_supported(ctx.target_triple) {
        return slow_call(ctx);
    }

    let fast_idx = ctx.new_block("rx_lit.fast");
    let bump_idx = ctx.new_block("rx_lit.bump");
    let slow_idx = ctx.new_block("rx_lit.slow");
    let merge_idx = ctx.new_block("rx_lit.merge");
    let fast_l = ctx.block_label(fast_idx);
    let bump_l = ctx.block_label(bump_idx);
    let slow_l = ctx.block_label(slow_idx);
    let merge_l = ctx.block_label(merge_idx);

    // Gate: the site words belong to the primary agent; a worker (or the
    // primary once any worker exists) takes the runtime birth.
    let header_word = {
        let blk = ctx.block();
        let workers = blk.load_atomic_seq_cst(I8, "@PERRY_METHOD_SITE_WORKERS_PRESENT", 1);
        let no_workers = blk.icmp_eq(I8, &workers, "0");
        let word1 = blk.gep(I8, &site, &[(I64, "8")]);
        let header_word = blk.load(I64, &word1);
        let primed = blk.icmp_ne(I64, &header_word, "0");
        let go = blk.and(I1, &no_workers, &primed);
        blk.cond_br(&go, &fast_l, &slow_l);
        header_word
    };

    // Bump allocate (the inline `new` sequence: offset stays 8-aligned).
    ctx.current_block = fast_idx;
    let total_size =
        crate::target_layout::inline_alloc_total_size_bytes(ctx.target_triple, REGEXP_FIELD_COUNT);
    let state = super::load_inline_arena_state(ctx);
    let (offset_ptr, offset, new_offset) = {
        let blk = ctx.block();
        let offset_ptr = blk.gep(I8, &state, &[(I64, "8")]);
        let offset = blk.load(I64, &offset_ptr);
        let new_offset = blk.add(I64, &offset, &total_size.to_string());
        let size_ptr = blk.gep(I8, &state, &[(I64, "16")]);
        let size = blk.load(I64, &size_ptr);
        let fits = blk.icmp_ule(I64, &new_offset, &size);
        blk.cond_br(&fits, &bump_l, &slow_l);
        (offset_ptr, offset, new_offset)
    };

    ctx.current_block = bump_idx;
    let raw = {
        let blk = ctx.block();
        // GC_STORE_AUDIT(INIT): inline arena bump offset is allocator metadata, not a JS heap edge.
        blk.store(I64, &new_offset, &offset_ptr);
        let data_base = blk.load(PTR, &state);
        blk.gep(I8, &data_base, &[(I64, &offset)])
    };
    let gc_packed =
        crate::target_layout::inline_alloc_gc_packed(ctx.target_triple, REGEXP_FIELD_COUNT)
            | PLAIN_ORDINARY_RESERVED_BITS;
    let birth_flags = super::inline_birth::flags(ctx, &state);
    let packed = super::inline_birth::header(ctx, &gc_packed.to_string(), &birth_flags);
    let handle = {
        let blk = ctx.block();
        let image0 = blk.next_reg();
        blk.emit_raw(format!(
            "{image0} = insertelement <2 x i64> poison, i64 {packed}, i32 0"
        ));
        let image = blk.next_reg();
        blk.emit_raw(format!(
            "{image} = insertelement <2 x i64> {image0}, i64 {header_word}, i32 1"
        ));
        // GC_STORE_AUDIT(INIT): inline headers initialize freshly allocated unpublished object storage.
        blk.emit_raw(format!("store <2 x i64> {image}, ptr {raw}, align 8"));
        let meta = blk.gep(I8, &raw, &[(I64, "16")]);
        // GC_STORE_AUDIT(INIT): fresh inline RegExp starts with no per-object meta record.
        blk.store(I64, "0", &meta);
        // Word 0 is read here, after the allocation and before any action
        // that could collect: the current address of the matcher data.
        let data = blk.load(I64, &site);
        let slot0 = blk.gep(I8, &raw, &[(I64, "24")]);
        // GC_STORE_AUDIT(INIT): newborn nursery RegExp's matcher slot; the birth seed below covers marking.
        blk.store(I64, &data, &slot0);
        let slot1 = blk.gep(I8, &raw, &[(I64, "32")]);
        // GC_STORE_AUDIT(INIT): newborn RegExp's lastIndex, the Number +0.
        blk.store(I64, "0", &slot1);
        let user = blk.gep(I8, &raw, &[(I64, "8")]);
        blk.ptrtoint(&user, I64)
    };
    // Seed only after every slot holds its value.
    super::inline_birth::finish(ctx, &raw, &birth_flags, &state);
    let fast_end = ctx.block().label.clone();
    ctx.block().br(&merge_l);

    ctx.current_block = slow_idx;
    let slow = slow_call(ctx);
    let slow_end = ctx.block().label.clone();
    ctx.block().br(&merge_l);

    ctx.current_block = merge_idx;
    ctx.block()
        .phi(I64, &[(&handle, &fast_end), (&slow, &slow_end)])
}
