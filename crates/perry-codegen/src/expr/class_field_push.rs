//! #11743: a single append through a declared array field.
//!
//! The transform binds the receiver to a real HIR local, so its slot is in the
//! function's GC roots. Check the method BEFORE the argument: either prove the
//! pristine builtin, or capture the actual method (including getters). The
//! builtin lane must not look up an own method installed by the argument.
use anyhow::Result;
use perry_hir::Expr;

use super::{lower_expr, FnCtx};
use crate::rooting::{self, Repr};
use crate::types::{DOUBLE, I1, I16, I64, I8, PTR};

pub(crate) fn lower(ctx: &mut FnCtx<'_>, receiver: &Expr, value: &Expr) -> Result<String> {
    let Expr::LocalGet(_) = receiver else {
        anyhow::bail!("push_field_single requires a bound receiver");
    };
    // #11394 supplies a whole-program proof, including imported modules. A
    // program that can patch `push` keeps lookup-first dispatch on every call.
    if crate::lower_call::property_get::patched_proto::guards(ctx, receiver, "push") {
        return lower_lookup_first(ctx, receiver, value);
    }
    let recv = lower_expr(ctx, receiver)?;
    let hdr = ctx.new_block("fieldpush.header");
    let builtin = ctx.new_block("fieldpush.builtin");
    let lookup = ctx.new_block("fieldpush.lookup");
    let argument = ctx.new_block("fieldpush.argument");
    let append = ctx.new_block("fieldpush.append");
    let call = ctx.new_block("fieldpush.call");
    let merge = ctx.new_block("fieldpush.merge");
    let hdr_label = ctx.block_label(hdr);
    let builtin_label = ctx.block_label(builtin);
    let lookup_label = ctx.block_label(lookup);
    let argument_label = ctx.block_label(argument);
    let append_label = ctx.block_label(append);
    let call_label = ctx.block_label(call);
    let merge_label = ctx.block_label(merge);
    let handle = {
        let blk = ctx.block();
        let bits = blk.bitcast_double_to_i64(&recv);
        let tag = blk.lshr(I64, &bits, "48");
        let pointer = blk.icmp_eq(I64, &tag, "32765");
        let handle = blk.and(I64, &bits, "281474976710655");
        let above = blk.icmp_ugt(I64, &handle, "1048575");
        let below = blk.icmp_ult(I64, &handle, "140737488355328");
        let range = blk.and(I1, &above, &below);
        let heap = blk.and(I1, &pointer, &range);
        blk.cond_br(&heap, &hdr_label, &lookup_label);
        handle
    };
    ctx.current_block = hdr;
    {
        let blk = ctx.block();
        let ptr = blk.inttoptr(I64, &handle);
        let kind_ptr = blk.gep(I8, &ptr, &[(I64, "-8")]);
        let kind = blk.load(I8, &kind_ptr);
        let array = blk.icmp_eq(I8, &kind, "1");
        let flags_ptr = blk.gep(I8, &ptr, &[(I64, "-7")]);
        let flags = blk.load(I8, &flags_ptr);
        let forwarded = blk.and(I8, &flags, "128");
        let live = blk.icmp_eq(I8, &forwarded, "0");
        let reserved_ptr = blk.gep(I8, &ptr, &[(I64, "-6")]);
        let reserved = blk.load(I16, &reserved_ptr);
        // NAMED_PROPS | ARRAY_DESCRIPTORS: no own method or method getter.
        let named = blk.and(I16, &reserved, "1280");
        let no_own = blk.icmp_eq(I16, &named, "0");
        let default_proto =
            super::array_proto_guard::emit_array_default_prototype_chain(blk, &reserved);
        let plain = blk.and(I1, &array, &live);
        let method = blk.and(I1, &no_own, &default_proto);
        let ok = blk.and(I1, &plain, &method);
        blk.cond_br(&ok, &builtin_label, &lookup_label);
    }
    // Resolve the method first, then merge BEFORE emitting the argument. In
    // particular, a large argument must not be copied into two code paths.
    ctx.current_block = builtin;
    ctx.block().br(&argument_label);
    ctx.current_block = lookup;
    let method = lookup_method(ctx, receiver)?;
    let lookup_end = ctx.block().label.clone();
    ctx.block().br(&argument_label);
    ctx.current_block = argument;
    let is_builtin = ctx
        .block()
        .phi(I1, &[("true", &builtin_label), ("false", &lookup_end)]);
    let method = ctx
        .block()
        .phi(DOUBLE, &[("0.0", &builtin_label), (&method, &lookup_end)]);
    rooting::with_rooted_group(ctx, 1, |ctx, group| {
        let method = group.adopt_emitted(ctx, Repr::Boxed, &method, true);
        let rooted_operands: [&perry_hir::Expr; 2] = [value, receiver];
        let (rooted_values, rooted_group) =
            crate::lower_call::lower_operand_list_rooted(ctx, &rooted_operands)?;
        let value = rooted_values[0].clone();
        let recv = rooted_values[1].clone();
        let method = group.reread_emitted(ctx, method);
        ctx.block().cond_br(&is_builtin, &append_label, &call_label);
        ctx.current_block = append;
        // One shared runtime implementation handles forwarding, capacity,
        // descriptors, integrity and GC barriers. Expanding ArrayPush here
        // multiplies that machinery at every unrolled/specialized call site.
        // The captured builtin decision survives mutations in the argument.
        let handle = super::unbox_to_i64(ctx.block(), &recv);
        ctx.block().call(
            I64,
            "js_array_push_f64_spec",
            &[(I64, &handle), (DOUBLE, &value)],
        );
        ctx.block().br(&merge_label);
        ctx.current_block = call;
        call_captured(ctx, &method, &recv, &value);
        ctx.block().br(&merge_label);
        ctx.current_block = merge;
        // This lowering is only used for discarded-result statements.
        let rooted_result = "0.0".into();
        rooted_group.release(ctx);
        Ok(rooted_result)
    })
}

fn lookup_method(ctx: &mut FnCtx<'_>, receiver: &Expr) -> Result<String> {
    lower_expr(
        ctx,
        &Expr::PropertyGet {
            object: Box::new(receiver.clone()),
            property: "push".into(),
            byte_offset: 0,
        },
    )
}

fn lower_lookup_first(ctx: &mut FnCtx<'_>, receiver: &Expr, value: &Expr) -> Result<String> {
    let method = lookup_method(ctx, receiver)?;
    rooting::with_rooted_group(ctx, 1, |ctx, group| {
        let method = group.adopt_emitted(ctx, Repr::Boxed, &method, true);
        let rooted_operands: [&perry_hir::Expr; 2] = [value, receiver];
        let (rooted_values, rooted_group) =
            crate::lower_call::lower_operand_list_rooted(ctx, &rooted_operands)?;
        let value = rooted_values[0].clone();
        let recv = rooted_values[1].clone();
        let method = group.reread_emitted(ctx, method);
        let rooted_result = call_captured(ctx, &method, &recv, &value);
        rooted_group.release(ctx);
        Ok(rooted_result)
    })
}

fn call_captured(ctx: &mut FnCtx<'_>, method: &str, recv: &str, value: &str) -> String {
    // An entry alloca cannot grow the stack on each loop iteration.
    let args = ctx.func.alloca_entry_array(DOUBLE, 1);
    let blk = ctx.block();
    let slot = blk.gep(DOUBLE, &args, &[(I64, "0")]);
    blk.store(DOUBLE, value, &slot);
    let this = blk.bitcast_double_to_i64(recv);
    blk.call(
        DOUBLE,
        "js_native_call_value",
        &[(DOUBLE, method), (I64, &this), (PTR, &args), (I64, "1")],
    )
}
