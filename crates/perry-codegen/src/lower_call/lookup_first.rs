//! #11910 for the method calls that have no method site: a Proxy receiver,
//! a computed key, a patched builtin-prototype name and the spread forms.
//!
//! ECMA-262 13.3.6.1 reads `recv[key]` BEFORE the arguments, so a getter, a
//! Proxy `get` trap or a nullish receiver's TypeError runs first. When the
//! arguments can observe that order (`args_may_observe_lookup`), the read
//! happens up front through `js_method_site_prepare` (no site slot): it
//! performs the spec Get when the read is observable and returns the value,
//! or returns `METHOD_SITE_BY_NAME` for a receiver whose read nothing can
//! observe, which keeps the caller's own by-name dispatch after the
//! arguments. Calls whose arguments cannot observe the order keep the
//! caller's fused lowering byte for byte.

use anyhow::Result;
use perry_hir::{CallArg, Expr};

use crate::expr::FnCtx;
use crate::rooting::{operand_may_collect, with_rooted_group, Repr};
use crate::types::{DOUBLE, I1, I64, PTR};

/// Where the method key comes from.
#[derive(Clone, Copy)]
pub(crate) enum Key<'a> {
    /// A static name (`recv.m(...)`).
    Name(&'a str),
    /// A computed key, lowered after the receiver (`recv[k](...)`).
    Value(&'a Expr),
}

/// The call's arguments.
#[derive(Clone, Copy)]
pub(crate) enum Args<'a> {
    List(&'a [Expr]),
    /// Regular and spread arguments, bundled into one array.
    Spread(&'a [CallArg]),
}

/// What the by-name half is handed: every value re-read below the arguments.
pub(crate) struct Parts {
    pub(crate) recv: String,
    /// The computed key (a `double`), for [`Key::Value`].
    pub(crate) key: Option<String>,
    /// The argument buffer and its length ([`Args::List`]).
    pub(crate) args_ptr: String,
    pub(crate) argc: String,
    /// The bundled argument array (`i64`, [`Args::Spread`]).
    pub(crate) array: String,
}

/// Can evaluating a spread call's arguments observe when the method is read?
/// A spread of anything but an array runs the iteration protocol (user code).
pub(crate) fn spread_args_may_observe_lookup(ctx: &FnCtx<'_>, args: &[CallArg]) -> bool {
    args.iter().any(|a| match a {
        CallArg::Expr(e) => {
            crate::expr::method_site::args_may_observe_lookup(ctx, std::slice::from_ref(e))
        }
        CallArg::Spread(e) => {
            !crate::type_analysis::is_array_expr(ctx, e)
                || crate::expr::method_site::args_may_observe_lookup(ctx, std::slice::from_ref(e))
        }
    })
}

/// Lower `object[key](args)` with the method read before the arguments; the
/// read is answered by `js_method_site_prepare` and `by_name` emits the
/// caller's own dispatch for the receivers whose read is not observable.
pub(crate) fn lower<'f>(
    ctx: &mut FnCtx<'f>,
    object: &Expr,
    key: Key<'_>,
    args: Args<'_>,
    by_name: impl FnOnce(&mut FnCtx<'f>, &Parts) -> String,
) -> Result<String> {
    let argc_static = match args {
        Args::List(a) => a.len(),
        Args::Spread(_) => 0,
    };
    let list_len = match args {
        Args::List(a) => a.len(),
        Args::Spread(_) => 0,
    };
    with_rooted_group(ctx, 3 + list_len, |ctx, group| {
        // The receiver is live across the read (which can run a getter) and
        // every argument; the key across the read and the arguments.
        let recv_i = group.lower(ctx, object, true)?;
        let key_i = match key {
            Key::Value(e) => Some(group.lower(ctx, e, true)?),
            Key::Name(_) => None,
        };
        let recv = group.reread(ctx, recv_i)?;
        let id = match key {
            Key::Name(name) => {
                let key_idx = ctx.strings.intern(name);
                let dispatch_global = ctx.strings.static_dispatch_global(key_idx);
                crate::strings::emit_static_dispatch_id(ctx.block(), &dispatch_global)
            }
            Key::Value(_) => {
                let key_box = group.reread(ctx, key_i.expect("computed key"))?;
                ctx.block().bitcast_double_to_i64(&key_box)
            }
        };
        let (named, value) = {
            let blk = ctx.block();
            let r = blk.call(
                DOUBLE,
                "js_method_site_prepare",
                &[
                    (PTR, "null"),
                    (DOUBLE, &recv),
                    (I64, &id),
                    (I64, &argc_static.to_string()),
                ],
            );
            let bits = blk.bitcast_double_to_i64(&r);
            // Either by-name answer (`BY_NAME` or `BY_NAME_DIRECT`).
            let either = blk.or(I64, &bits, "2");
            let named = blk.icmp_eq(
                I64,
                &either,
                &crate::runtime_abi::METHOD_SITE_BY_NAME_DIRECT.to_string(),
            );
            // The marker is not a JS value: root `undefined` in its place.
            let undefined = crate::nanbox::TAG_UNDEFINED.to_string();
            let vbits = blk.select(I1, &named, I64, &undefined, &bits);
            (named, blk.bitcast_i64_to_double(&vbits))
        };
        let collects = match args {
            Args::List(a) => a.iter().any(|e| operand_may_collect(ctx, e)),
            Args::Spread(_) => true,
        };
        let value_root = group.adopt_emitted(ctx, Repr::Boxed, &value, collects);
        match args {
            Args::List(list) => {
                let collects: Vec<bool> =
                    list.iter().map(|a| operand_may_collect(ctx, a)).collect();
                let mut idx = Vec::with_capacity(list.len());
                for (i, a) in list.iter().enumerate() {
                    idx.push(group.lower(ctx, a, collects[i + 1..].iter().any(|&c| c))?);
                }
                let recv = group.reread(ctx, recv_i)?;
                let key = key_i.map(|k| group.reread(ctx, k)).transpose()?;
                let value = group.reread_emitted(ctx, value_root);
                let mut vals = Vec::with_capacity(idx.len());
                for i in idx {
                    vals.push(group.reread(ctx, i)?);
                }
                let (args_ptr, argc) = crate::lower_call::emit_method_args_buffer(ctx, &vals);
                let parts = Parts {
                    recv,
                    key,
                    args_ptr,
                    argc,
                    array: "0".to_string(),
                };
                Ok(emit_diamond(ctx, &named, &value, &parts, by_name))
            }
            Args::Spread(list) => {
                crate::expr::call_spread::bundle_args_rooted(ctx, list, false, |ctx, acc| {
                    let recv = group.reread(ctx, recv_i)?;
                    let key = key_i.map(|k| group.reread(ctx, k)).transpose()?;
                    let value = group.reread_emitted(ctx, value_root);
                    let parts = Parts {
                        recv,
                        key,
                        args_ptr: "null".to_string(),
                        argc: "0".to_string(),
                        array: acc.to_string(),
                    };
                    Ok(emit_diamond(ctx, &named, &value, &parts, by_name))
                })
            }
        }
    })
}

/// `named ? by_name(parts) : call value with this = recv`.
fn emit_diamond<'f>(
    ctx: &mut FnCtx<'f>,
    named: &str,
    value: &str,
    parts: &Parts,
    by_name: impl FnOnce(&mut FnCtx<'f>, &Parts) -> String,
) -> String {
    let named_idx = ctx.new_block("lookup_first.by_name");
    let value_idx = ctx.new_block("lookup_first.value");
    let merge_idx = ctx.new_block("lookup_first.merge");
    let named_l = ctx.block_label(named_idx);
    let value_l = ctx.block_label(value_idx);
    let merge_l = ctx.block_label(merge_idx);
    ctx.block().cond_br(named, &named_l, &value_l);
    ctx.current_block = named_idx;
    let r_named = by_name(ctx, parts);
    let named_end = ctx.block().label.clone();
    ctx.block().br(&merge_l);
    ctx.current_block = value_idx;
    let r_value = if parts.array != "0" {
        ctx.block().call(
            DOUBLE,
            "js_method_site_call_value_apply",
            &[(DOUBLE, value), (DOUBLE, &parts.recv), (I64, &parts.array)],
        )
    } else {
        ctx.block().call(
            DOUBLE,
            "js_method_site_call_value",
            &[
                (DOUBLE, value),
                (DOUBLE, &parts.recv),
                (PTR, &parts.args_ptr),
                (I64, &parts.argc),
            ],
        )
    };
    let value_end = ctx.block().label.clone();
    ctx.block().br(&merge_l);
    ctx.current_block = merge_idx;
    ctx.block()
        .phi(DOUBLE, &[(&r_named, &named_end), (&r_value, &value_end)])
}
