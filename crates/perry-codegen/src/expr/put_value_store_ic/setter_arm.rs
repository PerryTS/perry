//! Setter candidates reuse the store site's one shared collecting exit.
//! The runtime setter consumer authenticates all shape, pair and lane facts.
use super::super::FnCtx;
use crate::types::I64;

pub(super) fn emit_setter_arm(
    ctx: &mut FnCtx<'_>,
    entry_idx: usize,
    packed_word: &str,
    packed_empty: i64,
    slow_label: &str,
    next_label: &str,
) {
    ctx.current_block = entry_idx;
    let empty = ctx
        .block()
        .icmp_eq(I64, packed_word, &packed_empty.to_string());
    ctx.block().cond_br(&empty, slow_label, next_label);
}
