//! Primary accessor validation is the shared guard program expanded into LLVM.
//! Saved receiver answers use the collecting runtime backend after a miss.
use super::super::FnCtx;
use crate::runtime_abi as abi;
use crate::types::{I1, I32, I64, I8, PTR};

/// Emit the arm starting at `entry_idx` (a fresh block the MRU compare's false
/// edge targets). Every failing test branches to `miss_label` (the front); the
/// call's result reaches `merge_label`. Returns `(value, end label)` for the
/// tower's merge phi.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_class_accessor_arm(
    ctx: &mut FnCtx<'_>,
    entry_idx: usize,
    packed_word: &str,
    packed_empty: i64,
    cache_slot_ref: &str,
    recv_biased: &str,
    recv_box: &str,
    header_bytes: i64,
    miss_label: &str,
    merge_label: &str,
) -> (String, String) {
    let cache_idx = ctx.new_block("pic.acc.cache");
    let recv_idx = ctx.new_block("pic.acc.recv");
    let kind_idx = ctx.new_block("pic.acc.kind");
    let holder_idx = ctx.new_block("pic.acc.holder");
    let lane_idx = ctx.new_block("pic.acc.lane");
    let call_idx = ctx.new_block("pic.acc.call");
    let cache_l = ctx.block_label(cache_idx);
    let recv_l = ctx.block_label(recv_idx);

    // A site that reads own data has a primed MRU word; its accessor reads
    // stay on the front and the slow call.
    ctx.current_block = entry_idx;
    {
        let blk = ctx.block();
        let empty = blk.icmp_eq(I64, packed_word, &packed_empty.to_string());
        blk.cond_br(&empty, &cache_l, miss_label);
    }

    // The full cache is allocated lazily; it is published with release order.
    ctx.current_block = cache_idx;
    let cache = {
        let blk = ctx.block();
        let cache = blk.load_atomic_acquire(PTR, cache_slot_ref, 8);
        let present = blk.icmp_ne(PTR, &cache, "null");
        blk.cond_br(&present, &recv_l, miss_label);
        cache
    };

    let mut guards = EmittedGuards {
        ctx,
        cache,
        recv_biased,
        header_bytes,
        miss_label,
        recv_idx,
        kind_idx,
        holder_idx,
        lane_idx,
        call_idx,
        kind: String::new(),
        holder: String::new(),
        pair: String::new(),
        getter: String::new(),
    };
    let Ok(()) = abi::accessor_guards::validate(&mut guards);
    let pair = guards.pair;
    let getter = guards.getter;

    // The getter runs user code: a versioned loop records its bailout here,
    // as on the collecting slow call.
    ctx.current_block = call_idx;
    crate::expr::emit_versioned_loop_callback_deopt(ctx);
    let blk = ctx.block();
    let code = blk.inttoptr(I64, &getter);
    let value = crate::expr::body_call::emit_accessor_getter_call(blk, &code, recv_box, &pair);
    let end = blk.label.clone();
    blk.br(merge_label);
    (value, end)
}

/// Emission backend: validation expands into main's primary guard sequence.
/// No runtime selector call, extra receiver conversion or cached result load.
struct EmittedGuards<'c, 'm> {
    ctx: &'c mut FnCtx<'m>,
    cache: String,
    recv_biased: &'c str,
    header_bytes: i64,
    miss_label: &'c str,
    recv_idx: usize,
    kind_idx: usize,
    holder_idx: usize,
    lane_idx: usize,
    call_idx: usize,
    kind: String,
    holder: String,
    pair: String,
    getter: String,
}

impl EmittedGuards<'_, '_> {
    fn word(&mut self, index: usize) -> String {
        let blk = self.ctx.block();
        let p = blk.gep(I64, &self.cache, &[(I64, &index.to_string())]);
        blk.load(I64, &p)
    }
}

impl abi::accessor_guards::AccessorGuards for EmittedGuards<'_, '_> {
    type Failure = core::convert::Infallible;

    fn receiver(&mut self) -> Result<(), Self::Failure> {
        self.ctx.current_block = self.recv_idx;
        let recv_word = self.word(abi::PIC_HOLDER_RECV_WORD);
        let next = self.ctx.block_label(self.kind_idx);
        let blk = self.ctx.block();
        let sid_ptr = crate::expr::receiver_range::emit_field_ptr(blk, self.recv_biased, 4);
        let sid = blk.load(I32, &sid_ptr);
        let primed = blk.trunc(I64, &recv_word, I32);
        let same = blk.icmp_eq(I32, &sid, &primed);
        blk.cond_br(&same, &next, self.miss_label);
        Ok(())
    }

    fn kind(&mut self) -> Result<(), Self::Failure> {
        self.ctx.current_block = self.kind_idx;
        self.kind = self.word(abi::PIC_HOLDER_KIND_WORD);
        let next = self.ctx.block_label(self.holder_idx);
        let blk = self.ctx.block();
        let bit = blk.and(I64, &self.kind, &abi::PIC_HOLDER_ACCESSOR_BIT.to_string());
        let accessor = blk.icmp_ne(I64, &bit, "0");
        blk.cond_br(&accessor, &next, self.miss_label);
        Ok(())
    }

    fn holder(&mut self) -> Result<(), Self::Failure> {
        self.ctx.current_block = self.holder_idx;
        self.holder = self.word(abi::PIC_HOLDER_OBJ_WORD);
        let holder_shape = self.word(abi::PIC_HOLDER_SHAPE_WORD);
        let next = self.ctx.block_label(self.lane_idx);
        let blk = self.ctx.block();
        let sid_addr = blk.add(I64, &self.holder, "4");
        let sid_ptr = blk.inttoptr(I64, &sid_addr);
        let sid = blk.load(I32, &sid_ptr);
        let primed = blk.trunc(I64, &holder_shape, I32);
        let same = blk.icmp_eq(I32, &sid, &primed);
        blk.cond_br(&same, &next, self.miss_label);
        Ok(())
    }

    fn callable(&mut self) -> Result<(), Self::Failure> {
        self.ctx.current_block = self.lane_idx;
        self.pair = self.word(abi::PIC_HOLDER_PAIR_WORD);
        self.getter = self.word(abi::PIC_HOLDER_GETTER_WORD);
        let inline_idx = self.ctx.new_block("pic.acc.inline");
        let next = self.ctx.block_label(inline_idx);
        let blk = self.ctx.block();
        let workers = blk.load_atomic_seq_cst(I8, "@PERRY_METHOD_SITE_WORKERS_PRESENT", 1);
        let workers = blk.zext(I8, &workers, I32);
        let no_workers = blk.icmp_eq(I32, &workers, "0");
        // Publication stores getter=0 for setter-only and spill answers.
        // A nonzero getter therefore proves that the next load is inline.
        let has_getter = blk.icmp_ne(I64, &self.getter, "0");
        let ok = blk.and(I1, &no_workers, &has_getter);
        blk.cond_br(&ok, &next, self.miss_label);
        self.ctx.current_block = inline_idx;
        Ok(())
    }

    fn lane(&mut self) -> Result<(), Self::Failure> {
        let next = self.ctx.block_label(self.call_idx);
        let blk = self.ctx.block();
        let slot = blk.and(I64, &self.kind, "4294967295");
        let base_addr = blk.add(I64, &self.holder, &self.header_bytes.to_string());
        let base = blk.inttoptr(I64, &base_addr);
        let lane_ptr = blk.gep(I64, &base, &[(I64, &slot)]);
        let lane = blk.load(I64, &lane_ptr);
        let tagged = blk.or(I64, &self.pair, crate::nanbox::POINTER_TAG_I64);
        let same = blk.icmp_eq(I64, &lane, &tagged);
        blk.cond_br(&same, &next, self.miss_label);
        Ok(())
    }
}
