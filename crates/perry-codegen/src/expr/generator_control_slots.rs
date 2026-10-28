//! Native scalar operations for controls named by LinkGeneratorPrototype.
//! The single private resume closure proves these slots are inaccessible to
//! JS and never contain pointers. Their stored representation remains an
//! ordinary NaN-boxed capture so existing tracing and capture initialization
//! need no second layout. User locals never select this path by annotation.
use super::*;

pub(super) fn is_fixed(ctx: &FnCtx<'_>, id: u32) -> bool {
    ctx.fixed_generator_state
        && !ctx.boxed_vars.contains(&id)
        && (ctx.closure_captures.contains_key(&id) || ctx.locals.contains_key(&id))
        && (ctx.compiler_private_async_i32_control_locals.contains(&id)
            || ctx.compiler_private_async_i1_control_locals.contains(&id))
}

pub(super) fn address(ctx: &mut FnCtx<'_>, id: u32) -> Result<String> {
    // The private operation parameter stays in its ordinary DOUBLE alloca.
    // Its explicit transform control id and protocol ABI prove the scalar.
    if let Some(slot) = ctx.locals.get(&id).cloned() {
        if !ctx.closure_captures.contains_key(&id) {
            return Ok(slot);
        }
    }
    let index = ctx.closure_captures[&id];
    let closure = current_closure_ptr_value(ctx, "fixed generator control")?;
    let index = super::private_resume_frame::slot_index(ctx, index);
    let offset = super::private_resume_frame::slot_offset(ctx, &index);
    Ok(ctx.block().add(I64, &closure, &offset.to_string()))
}

pub(super) fn load_i32(ctx: &mut FnCtx<'_>, id: u32, addr: &str) -> String {
    if !is_fixed(ctx, id) {
        return load_async_i32_control_cell(ctx, addr);
    }
    let ptr = if ctx.locals.contains_key(&id) && !ctx.closure_captures.contains_key(&id) {
        addr.to_string()
    } else {
        ctx.block().inttoptr(I64, addr)
    };
    let value = ctx.block().load(DOUBLE, &ptr);
    ctx.block().fptosi(DOUBLE, &value, I32)
}

pub(super) fn load_i1(ctx: &mut FnCtx<'_>, id: u32, addr: &str) -> String {
    if !is_fixed(ctx, id) {
        return load_async_i1_control_cell(ctx, addr);
    }
    let ptr = ctx.block().inttoptr(I64, addr);
    let bits = ctx.block().load(I64, &ptr);
    ctx.block().icmp_eq(I64, &bits, crate::nanbox::TAG_TRUE_I64)
}

pub(super) fn store_i32(ctx: &mut FnCtx<'_>, id: u32, addr: &str, value: &str) -> Result<()> {
    if !is_fixed(ctx, id) {
        store_async_i32_control_cell(ctx, addr, value);
        return Ok(());
    }
    // Value evaluation may collect. Re-read the rooted closure after it.
    let addr = address(ctx, id)?;
    let ptr = if ctx.locals.contains_key(&id) && !ctx.closure_captures.contains_key(&id) {
        addr
    } else {
        ctx.block().inttoptr(I64, &addr)
    };
    let value = ctx.block().sitofp(I32, value, DOUBLE);
    // GC_STORE_AUDIT(POINTER_FREE): compiler-owned i32 control, scalar from birth.
    ctx.block().store(DOUBLE, &value, &ptr);
    Ok(())
}

pub(super) fn store_i1(ctx: &mut FnCtx<'_>, id: u32, addr: &str, value: &str) -> Result<()> {
    if !is_fixed(ctx, id) {
        store_async_i1_control_cell(ctx, addr, value);
        return Ok(());
    }
    let addr = address(ctx, id)?;
    let ptr = ctx.block().inttoptr(I64, &addr);
    let bits = ctx.block().select(
        I1,
        value,
        I64,
        crate::nanbox::TAG_TRUE_I64,
        crate::nanbox::TAG_FALSE_I64,
    );
    // GC_STORE_AUDIT(POINTER_FREE): compiler-owned boolean control, scalar from birth.
    ctx.block().store(I64, &bits, &ptr);
    Ok(())
}
