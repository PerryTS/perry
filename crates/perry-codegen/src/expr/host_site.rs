//! HOSTSURF H5 (§3.2): a proven host site (`ElisionFacts::host_member_calls`,
//! `host_accessor_sites`) is the direct call of the provider's endpoint:
//!
//! ```text
//!   ; r evaluated; nullable sites: the one nullish edge throws the TypeError
//!   H = perry_host_prototype_v1(provider_id, object_index)  ; registry load
//!   F = perry_host_own_member_v1(H, key, part)              ; ONE own-slot read
//!   v = perry_binding_native_call_v1(F, r, args, n)
//! ```
//!
//! No chain walk, no interceptor, no host-kind probe, and no check: the facts
//! prove every receiver inherits the key from that registered prototype, that
//! nothing writes it, and so that F is the installed native function (the
//! method; for an accessor, its getter or setter, §3.4: the method path, no
//! realm entry). `perry_binding_native_call_v1` is the provider runtime's
//! export (feather: `native_function_operation_with_arguments`); it roots F,
//! r and the arguments, runs the termination check and throws the status-1
//! value. The registry and the own-slot read are perry-runtime's
//! (`host_registry.rs`, `perry_host_own_member_v1`).
use anyhow::Result;
use perry_hir::Expr;

use crate::codegen::{AccessorKind, FactsHostCall};
use crate::nanbox::POINTER_MASK_I64;
use crate::types::{DOUBLE, I32, I64, PTR};

use super::FnCtx;

/// The registry's own-slot part: the data value (a method).
const PART_METHOD: u32 = 0;
/// The own accessor's getter.
const PART_GETTER: u32 = 1;
/// The own accessor's setter.
const PART_SETTER: u32 = 2;

/// The provider's endpoint call (feather's `perry_binding_native_call_v1`).
pub(crate) const NATIVE_CALL: &str = "perry_binding_native_call_v1";

/// H and F: the registry load and the one own-slot read of `property`.
fn emit_member(ctx: &mut FnCtx<'_>, host: &FactsHostCall, property: &str, part: u32) -> String {
    let key_idx = ctx.strings.intern(property);
    let key_handle_global = format!("@{}", ctx.strings.entry(key_idx).handle_global);
    let blk = ctx.block();
    let proto = blk.call(
        DOUBLE,
        "perry_host_prototype_v1",
        &[
            (I32, &host.provider_id.to_string()),
            (I32, &host.object_index.to_string()),
        ],
    );
    let key_box = blk.load(DOUBLE, &key_handle_global);
    let key_bits = blk.bitcast_double_to_i64(&key_box);
    let key_handle = blk.and(I64, &key_bits, POINTER_MASK_I64);
    blk.call(
        DOUBLE,
        "perry_host_own_member_v1",
        &[(DOUBLE, &proto), (I64, &key_handle), (I32, &part.to_string())],
    )
}

/// `perry_binding_native_call_v1(F, r, args, n)`: the arguments in a stack
/// buffer the provider's shim roots before anything can collect.
fn emit_native_call(ctx: &mut FnCtx<'_>, f: &str, recv: &str, args: &[String]) -> String {
    let (args_ptr, argc) = crate::lower_call::emit_method_args_buffer(ctx, args);
    let blk = ctx.block();
    let f_bits = blk.bitcast_double_to_i64(f);
    let recv_bits = blk.bitcast_double_to_i64(recv);
    blk.call(
        DOUBLE,
        NATIVE_CALL,
        &[(I64, &f_bits), (I64, &recv_bits), (PTR, &args_ptr), (I32, &argc)],
    )
}

/// The site's host fact, when it has one.
fn host_call_of(ctx: &FnCtx<'_>, offset: u32, property: &str) -> Option<FactsHostCall> {
    if ctx.wpfacts_off {
        return None;
    }
    ctx.wpfacts
        .host_member_calls
        .get(&(offset, property.to_string()))
        .cloned()
}

fn host_accessor_of(
    ctx: &FnCtx<'_>,
    offset: u32,
    property: &str,
    kind: AccessorKind,
) -> Option<FactsHostCall> {
    if ctx.wpfacts_off {
        return None;
    }
    ctx.wpfacts
        .host_accessor_sites
        .get(&(offset, property.to_string(), kind))
        .cloned()
}

/// `r.k(args)` at a `Verdict::HostMember` call site: the receiver, its
/// nullish edge (before the arguments: the [[Get]] of `k` throws first,
/// 13.3.6.1), the arguments, then H, F and the endpoint call.
pub(crate) fn try_lower_host_call(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    args: &[Expr],
) -> Result<Option<String>> {
    let Expr::PropertyGet {
        object,
        property,
        byte_offset,
        ..
    } = callee
    else {
        return Ok(None);
    };
    let Some(host) = host_call_of(ctx, *byte_offset, property) else {
        return Ok(None);
    };
    crate::facts_consumed::used();
    let mut roots = crate::rooting::open_rooted_group(1 + args.len());
    let recv_idx = roots.lower(ctx, object, true)?;
    if host.nullable {
        let recv_box = roots.reread(ctx, recv_idx)?;
        super::wpfacts_access::emit_nullish_throw_test(
            ctx,
            object,
            &recv_box,
            property,
            *byte_offset,
            None,
        );
    }
    let mut arg_idxs = Vec::with_capacity(args.len());
    for a in args {
        arg_idxs.push(roots.lower(ctx, a, true)?);
    }
    let f = emit_member(ctx, &host, property, PART_METHOD);
    let recv_box = roots.reread(ctx, recv_idx)?;
    let mut lowered: Vec<String> = Vec::with_capacity(args.len());
    for &idx in &arg_idxs {
        lowered.push(roots.reread(ctx, idx)?);
    }
    let value = emit_native_call(ctx, &f, &recv_box, &lowered);
    roots.release(ctx);
    Ok(Some(value))
}

/// `r.k` at a `Verdict::HostGet` read (`kind`: the ordinary read or a
/// compound's GetValue): the getter, called with no arguments.
pub(crate) fn try_lower_host_getter(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    offset: u32,
    kind: AccessorKind,
    nullish_tested: bool,
) -> Result<Option<String>> {
    let Some(host) = host_accessor_of(ctx, offset, property, kind) else {
        return Ok(None);
    };
    crate::facts_consumed::used();
    let mut roots = crate::rooting::open_rooted_group(1);
    let recv_idx = roots.lower(ctx, object, true)?;
    if host.nullable && !nullish_tested {
        let recv_box = roots.reread(ctx, recv_idx)?;
        super::wpfacts_access::emit_nullish_throw_test(ctx, object, &recv_box, property, offset, None);
    }
    let f = emit_member(ctx, &host, property, PART_GETTER);
    let recv_box = roots.reread(ctx, recv_idx)?;
    let value = emit_native_call(ctx, &f, &recv_box, &[]);
    roots.release(ctx);
    Ok(Some(value))
}

/// `r.k = v` at a `Verdict::HostSet` store: the setter, called with the
/// value; the expression's value is the stored value (13.15.2). A nullish
/// receiver takes the complete [[Set]] after the value was evaluated, which
/// throws the TypeError (`emit_nullish_store_test`).
pub(crate) fn try_lower_host_setter(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    value: &Expr,
    offset: u32,
) -> Result<Option<String>> {
    let Some(host) = host_accessor_of(ctx, offset, property, AccessorKind::Set) else {
        return Ok(None);
    };
    crate::facts_consumed::used();
    let strict = ctx.is_strict_fn;
    let mut roots = crate::rooting::open_rooted_group(2);
    let recv_idx = roots.lower(ctx, object, true)?;
    let val_idx = roots.lower(ctx, value, true)?;
    if host.nullable {
        let recv_box = roots.reread(ctx, recv_idx)?;
        let val_box = roots.reread(ctx, val_idx)?;
        super::property_set::emit_nullish_store_test(
            ctx, object, property, &recv_box, &val_box, strict,
        );
    }
    let f = emit_member(ctx, &host, property, PART_SETTER);
    let recv_box = roots.reread(ctx, recv_idx)?;
    let val_box = roots.reread(ctx, val_idx)?;
    let _ = emit_native_call(ctx, &f, &recv_box, std::slice::from_ref(&val_box));
    roots.release(ctx);
    Ok(Some(val_box))
}
