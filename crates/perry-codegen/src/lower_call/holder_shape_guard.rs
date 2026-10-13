//! Guard compiler body candidates with the actual lazy holder shapes.
use crate::expr::FnCtx;
use crate::types::{I1, I32, I64, I8, PTR};

pub(crate) fn method_guard(ctx: &mut FnCtx<'_>, class: &str, method: &str) -> String {
    let Some(chain) = crate::codegen::static_prototype::chain(ctx, class, method) else {
        return "false".into();
    };
    holders_guard(ctx, &chain)
}

pub(crate) fn holders_guard(ctx: &mut FnCtx<'_>, chain: &[(u32, u32)]) -> String {
    if crate::target_layout::target_is_ilp32(ctx.target_triple) {
        return "false".into();
    }
    let dir = crate::expr::agent_ptr::emit_agent_ptr(
        ctx,
        crate::runtime_abi::AGENT_PTR_CLASS_VALUES,
        "perry_class_value_dir_cell",
    );
    let mut result = "true".to_string();
    for &(cid, expected) in chain {
        let ok = holder_matches(ctx, &dir, cid, expected);
        result = ctx.block().and(I1, &result, &ok);
    }
    result
}

/// Empty directory entries/links prove pristine: a mutation first materializes
/// the prototype. A present holder must still carry the complete candidate P.
fn holder_matches(ctx: &mut FnCtx<'_>, dir: &str, cid: u32, expected: u32) -> String {
    let page = ctx.new_block("holder.page");
    let ctor = ctx.new_block("holder.constructor");
    let link = ctx.new_block("holder.link");
    let shape = ctx.new_block("holder.shape");
    let merge = ctx.new_block("holder.merge");
    let pl = ctx.block_label(page);
    let cl = ctx.block_label(ctor);
    let ll = ctx.block_label(link);
    let sl = ctx.block_label(shape);
    let ml = ctx.block_label(merge);
    let start = ctx.block().label.clone();
    let pages = ctx.block().load(PTR, dir);
    let at = ctx.block().gep(I8, dir, &[(I64, "8")]);
    let len = ctx.block().load(I64, &at);
    let present = ctx.block().icmp_ult(I64, &(cid >> 8).to_string(), &len);
    ctx.block().cond_br(&present, &pl, &ml);
    ctx.current_block = page;
    let at = ctx
        .block()
        .gep(PTR, &pages, &[(I64, &(cid >> 8).to_string())]);
    let p = ctx.block().load(PTR, &at);
    let present = ctx.block().icmp_ne(PTR, &p, "null");
    ctx.block().cond_br(&present, &cl, &ml);
    ctx.current_block = ctor;
    let at = ctx.block().gep(PTR, &p, &[(I64, &(cid & 255).to_string())]);
    let c = ctx.block().load(PTR, &at);
    let present = ctx.block().icmp_ne(PTR, &c, "null");
    ctx.block().cond_br(&present, &ll, &ml);
    ctx.current_block = link;
    let off = crate::runtime_abi::CLOSURE_HEADER_SIZE
        + crate::runtime_abi::CLASS_PROTOTYPE_LINK_CAPTURE * 8;
    let at = ctx.block().gep(I8, &c, &[(I64, &off.to_string())]);
    let bits = ctx.block().load(I64, &at);
    let tag = ctx.block().lshr(I64, &bits, "48");
    let present = ctx
        .block()
        .icmp_eq(I64, &tag, super::method_override::POINTER_TAG_HI16);
    ctx.block().cond_br(&present, &sl, &ml);
    ctx.current_block = shape;
    let raw = ctx.block().and(I64, &bits, crate::nanbox::POINTER_MASK_I64);
    let ptr = ctx.block().inttoptr(I64, &raw);
    let at = ctx.block().gep(I8, &ptr, &[(I64, "4")]);
    let live = ctx.block().load(I32, &at);
    let matches = ctx.block().icmp_eq(I32, &live, &expected.to_string());
    ctx.block().br(&ml);
    ctx.current_block = merge;
    ctx.block().phi(
        I1,
        &[
            ("true", &start),
            ("true", &pl),
            ("true", &cl),
            ("true", &ll),
            (&matches, &sl),
        ],
    )
}
