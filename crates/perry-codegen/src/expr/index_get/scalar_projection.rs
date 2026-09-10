//! Optional dotted scalar read on the dynamic index dispatcher's lazy-brand edge.
//! No managed allocation, poll, or callback precedes the cold runtime fallback.

use super::FnCtx;
use crate::types::{DOUBLE, I1, I16, I32, I64, I8, PTR};

pub(crate) struct ScalarProjection {
    property: String,
    pub merge_block: Option<usize>,
    pub incoming: Vec<(String, String)>,
}

pub(super) struct ProjectionEdge {
    pub entry: String,
    pub exposed: (String, String),
    pub materialized: (String, String),
}

impl ScalarProjection {
    pub fn new(property: &str) -> Self {
        Self {
            property: property.into(),
            merge_block: None,
            incoming: Vec::new(),
        }
    }

    /// Existing dispatch proved pointer tag/address band, numeric integer index
    /// and no forwarding. This edge follows the ordinary Array/Object brands.
    /// The caller admits only 64-bit targets; runtime const asserts pin offsets.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn emit(
        &mut self,
        ctx: &mut FnCtx<'_>,
        receiver: &str,
        index: &str,
        raw: &str,
        index_i64: &str,
        gc_type: &str,
        fallback: &str,
        element_merge: &str,
        array_guard: &str,
    ) -> ProjectionEdge {
        let kind = ctx.new_block("json.scalar.kind");
        let guard = ctx.new_block("json.scalar.guard");
        let cache_guard = ctx.new_block("json.scalar.cache_guard");
        let materialized_guard = ctx.new_block("json.scalar.materialized_guard");
        let materialized_ready = ctx.new_block("json.scalar.materialized_ready");
        let bitmap = ctx.new_block("json.scalar.bitmap");
        let exposed = ctx.new_block("json.scalar.exposed");
        let memo = ctx.new_block("json.scalar.memo");
        let memo_slot = ctx.new_block("json.scalar.memo_slot");
        let memo_hit = ctx.new_block("json.scalar.memo_hit");
        let cold = ctx.new_block("json.scalar.cold");
        let merge = ctx.new_block("json.scalar.merge");
        self.merge_block = Some(merge);
        let kind_label = ctx.block_label(kind);
        let guard_label = ctx.block_label(guard);
        let cache_guard_label = ctx.block_label(cache_guard);
        let materialized_guard_label = ctx.block_label(materialized_guard);
        let materialized_ready_label = ctx.block_label(materialized_ready);
        let bitmap_label = ctx.block_label(bitmap);
        let exposed_label = ctx.block_label(exposed);
        let memo_label = ctx.block_label(memo);
        let memo_slot_label = ctx.block_label(memo_slot);
        let memo_hit_label = ctx.block_label(memo_hit);
        let cold_label = ctx.block_label(cold);
        let merge_label = ctx.block_label(merge);

        ctx.current_block = kind;
        let lazy = ctx.block().icmp_eq(I8, gc_type, "9");
        // Keep the optional JSON arm cold when LLVM folds the preceding brand
        // tests into a switch. Source order alone allowed its comparison to
        // move ahead of ordinary Array/Object reads in the optimized code.
        let lazy = ctx
            .block()
            .call(I1, "llvm.expect.i1", &[(I1, &lazy), (I1, "false")]);
        ctx.block().cond_br(&lazy, &guard_label, fallback);

        ctx.current_block = guard;
        let hdr = ctx.block().inttoptr(I64, raw);
        let magic_ptr = ctx.block().gep(I8, &hdr, &[(I64, "4")]);
        let magic = ctx.block().load(I32, &magic_ptr);
        let magic_ok = ctx.block().icmp_eq(I32, &magic, "1280989249"); // LZXA
        let mat_ptr = ctx.block().gep(I8, &hdr, &[(I64, "32")]);
        let mat = ctx.block().load(PTR, &mat_ptr);
        let unmaterialized = ctx.block().icmp_eq(PTR, &mat, "null");
        let flags_ptr = ctx.block().gep(I8, &hdr, &[(I64, "-6")]);
        let flags = ctx.block().load(I16, &flags_ptr);
        let desc = ctx.block().and(I16, &flags, "3072");
        let no_desc = ctx.block().icmp_eq(I16, &desc, "0");
        let header_ok = ctx.block().and(I1, &magic_ok, &no_desc);
        let has_materialized = ctx.block().icmp_ne(PTR, &mat, "null");
        let inspect_materialized = ctx.block().and(I1, &header_ok, &has_materialized);
        ctx.block().cond_br(
            &inspect_materialized,
            &materialized_guard_label,
            &cache_guard_label,
        );

        ctx.current_block = materialized_guard;
        // `materialized` is a managed edge installed by the runtime. Its live
        // target can have a growth forwarding header, so validate that header
        // before sharing the ordinary Array guard. A forwarded target keeps
        // the boxed fallback, which resolves and refreshes the owner's edge.
        // No call or collection occurs while this borrowed edge is live.
        let mat_raw = ctx.block().ptrtoint(&mat, I64);
        let mat_kind_ptr = ctx.block().gep(I8, &mat, &[(I64, "-8")]);
        let mat_kind = ctx.block().load(I8, &mat_kind_ptr);
        let mat_is_array = ctx.block().icmp_eq(I8, &mat_kind, "1");
        let mat_flags_ptr = ctx.block().gep(I8, &mat, &[(I64, "-7")]);
        let mat_flags = ctx.block().load(I8, &mat_flags_ptr);
        let mat_forwarded = ctx.block().and(I8, &mat_flags, "128");
        let mat_not_forwarded = ctx.block().icmp_eq(I8, &mat_forwarded, "0");
        let mat_ok = ctx.block().and(I1, &mat_is_array, &mat_not_forwarded);
        ctx.block()
            .cond_br(&mat_ok, &materialized_ready_label, fallback);

        ctx.current_block = materialized_ready;
        // Preserve resolve_materialized_array's length refresh: another alias
        // can mutate the backing array while this receiver stays a lazy header.
        let materialized_length = ctx.block().load(I32, &mat);
        // GC_STORE_AUDIT(POINTER_FREE): cached length is a u32, never a heap edge.
        ctx.block().store(I32, &materialized_length, &hdr);
        let mat_predecessor = ctx.block().label.clone();
        ctx.block().br(array_guard);

        ctx.current_block = cache_guard;
        let len = ctx.block().load(I32, &hdr);
        let len = ctx.block().zext(I32, &len, I64);
        let in_bounds = ctx.block().icmp_ult(I64, index_i64, &len);
        let cache_ptr = ctx.block().gep(I8, &hdr, &[(I64, "40")]);
        let cache = ctx.block().load(PTR, &cache_ptr);
        let bitmap_ptr = ctx.block().gep(I8, &hdr, &[(I64, "48")]);
        let bitmap_base = ctx.block().load(PTR, &bitmap_ptr);
        let cache_present = ctx.block().icmp_ne(PTR, &cache, "null");
        let bitmap_present = ctx.block().icmp_ne(PTR, &bitmap_base, "null");
        let ok = ctx.block().and(I1, &in_bounds, &header_ok);
        let ok = ctx.block().and(I1, &ok, &unmaterialized);
        let ok = ctx.block().and(I1, &ok, &cache_present);
        let ok = ctx.block().and(I1, &ok, &bitmap_present);
        ctx.block().cond_br(&ok, &bitmap_label, fallback);

        ctx.current_block = bitmap;
        let word_index = ctx.block().lshr(I64, index_i64, "6");
        let word_ptr = ctx.block().gep(I64, &bitmap_base, &[(I64, &word_index)]);
        let word = ctx.block().load(I64, &word_ptr);
        let bit = ctx.block().and(I64, index_i64, "63");
        let mask = ctx.block().shl(I64, "1", &bit);
        let selected = ctx.block().and(I64, &word, &mask);
        let is_exposed = ctx.block().icmp_ne(I64, &selected, "0");
        let slot_ptr = ctx.block().gep(I64, &cache, &[(I64, index_i64)]);
        ctx.block()
            .cond_br(&is_exposed, &exposed_label, &memo_label);

        ctx.current_block = exposed;
        // The exposed record wins over every scalar memo. Feed it to the same
        // element merge/PIC as an ordinary index load, preserving identity and
        // allowing its property lookup to invoke getters or collect normally.
        let value = ctx.block().load(DOUBLE, &slot_ptr);
        let exposed_end = ctx.block().label.clone();
        ctx.block().br(element_merge);

        ctx.current_block = memo;
        let mut packed = [0u8; 8];
        packed[0] = self.property.len() as u8 + 1;
        packed[1..self.property.len() + 1].copy_from_slice(self.property.as_bytes());
        let property = u64::from_le_bytes(packed).to_string();
        let property_ptr = ctx.block().gep(I8, &hdr, &[(I64, "80")]);
        let chosen = ctx.block().load(I64, &property_ptr);
        let matches = ctx.block().icmp_eq(I64, &chosen, &property);
        let choose = ctx.block().icmp_eq(I64, &chosen, "0");
        let same_or_empty = ctx.block().or(I1, &matches, &choose);
        ctx.block()
            .cond_br(&same_or_empty, &memo_slot_label, fallback);

        ctx.current_block = memo_slot;
        let bits = ctx.block().load(I64, &slot_ptr);
        let present = ctx.block().icmp_ne(I64, &bits, "0");
        let memo_valid = ctx.block().and(I1, &matches, &present);
        ctx.block()
            .cond_br(&memo_valid, &memo_hit_label, &cold_label);

        ctx.current_block = memo_hit;
        let is_zero = ctx
            .block()
            .icmp_eq(I64, &bits, &crate::nanbox::INT32_TAG.to_string());
        let decoded = ctx.block().select(I1, &is_zero, I64, "0", &bits);
        let scalar = ctx.block().bitcast_i64_to_double(&decoded);
        self.incoming.push((scalar, ctx.block().label.clone()));
        ctx.block().br(&merge_label);

        ctx.current_block = cold;
        let key = ctx.strings.intern(&self.property);
        let key_bytes = format!("@{}", ctx.strings.entry(key).bytes_global);
        let projected = ctx.block().call(
            DOUBLE,
            "js_json_lazy_index_scalar",
            &[
                (DOUBLE, receiver),
                (DOUBLE, index),
                (PTR, &key_bytes),
                (I64, &self.property.len().to_string()),
            ],
        );
        let projected_bits = ctx.block().bitcast_double_to_i64(&projected);
        let missed = ctx
            .block()
            .icmp_eq(I64, &projected_bits, crate::nanbox::TAG_HOLE_I64);
        self.incoming.push((projected, ctx.block().label.clone()));
        ctx.block().cond_br(&missed, fallback, &merge_label);
        ProjectionEdge {
            entry: kind_label,
            exposed: (value, exposed_end),
            materialized: (mat_raw, mat_predecessor),
        }
    }
}
