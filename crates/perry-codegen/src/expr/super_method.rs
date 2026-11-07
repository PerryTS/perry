//! SuperMethodCall / SuperPropertyGet / FsReadFileBinary.
//!
//! Extracted from `expr/mod.rs` to keep that file under the 2000-line cap.
//! Pure mechanical move — match arm bodies are verbatim copies, called from
//! `lower_expr`'s outer dispatch.

use anyhow::{anyhow, Result};
use perry_hir::Expr;

use crate::types::{DOUBLE, I32, I64, PTR};

use super::{lower_expr, nanbox_pointer_inline, FnCtx};

pub(crate) fn lower(ctx: &mut FnCtx<'_>, expr: &Expr) -> Result<String> {
    match expr {
        Expr::ClassMethodHome {
            class_name,
            is_static,
        } => {
            let owner = if let Some(slot) = ctx.current_closure_slot.clone() {
                let callee = ctx.block().load(I64, &slot);
                super::nanbox_pointer_inline(ctx.block(), &callee)
            } else if let Some(callee) = ctx.current_closure_ptr.clone() {
                super::nanbox_pointer_inline(ctx.block(), &callee)
            } else {
                let cid =
                    ctx.class_ids.get(class_name).copied().ok_or_else(|| {
                        anyhow!("missing lexical class identity for {class_name}")
                    })?;
                // A direct declaration body has one lexical constructor.
                // Read it through the shared holder operation; no site cache.
                super::emit_class_value(ctx.block(), cid)
            };
            Ok(ctx.block().call(
                DOUBLE,
                "js_class_method_home",
                &[(DOUBLE, &owner), (I32, &(*is_static as u32).to_string())],
            ))
        }
        Expr::SuperPropertyGet { property } => {
            let home = lexical_class_home(ctx)?;
            lower(
                ctx,
                &Expr::ObjectSuperPropertyGet {
                    home: Box::new(home),
                    key: Box::new(Expr::String(property.clone())),
                    receiver: Box::new(Expr::This),
                },
            )
        }
        Expr::SuperPropertySet { key, value, .. } => {
            let home = lexical_class_home(ctx)?;
            lower(
                ctx,
                &Expr::ObjectSuperPropertySet {
                    home: Box::new(home),
                    key: key.clone(),
                    value: value.clone(),
                    receiver: Box::new(Expr::This),
                },
            )
        }
        Expr::SuperMethodCall { method, args } => {
            match super_target(ctx, method) {
                Ok(target) => return lower_proven_super_call(ctx, method, args, target),
                Err(reason) => record_super_site(ctx, method, "super_generic", reason),
            }
            let home = lexical_class_home(ctx)?;
            lower(
                ctx,
                &Expr::ObjectSuperMethodCall {
                    home: Box::new(home),
                    key: Box::new(Expr::String(method.clone())),
                    receiver: Box::new(Expr::This),
                    args: args.clone(),
                },
            )
        }
        Expr::SuperMethodCallSpread { method, args } => {
            let home = lexical_class_home(ctx)?;
            super::call_spread::lower(
                ctx,
                &Expr::CallSpread {
                    byte_offset: 0,
                    byte_end: 0,
                    callee: Box::new(Expr::ObjectSuperPropertyGet {
                        home: Box::new(home),
                        key: Box::new(Expr::String(method.clone())),
                        receiver: Box::new(Expr::This),
                    }),
                    args: args.clone(),
                    type_args: Vec::new(),
                },
            )
        }

        Expr::ObjectSuperPropertyGet {
            home,
            key,
            receiver,
        } => {
            let rooted_operands: [&perry_hir::Expr; 3] = [home, key, receiver];
            let (rooted_values, rooted_group) =
                crate::lower_call::lower_operand_list_rooted(ctx, &rooted_operands)?;
            let home_v = rooted_values[0].clone();
            let key_v = rooted_values[1].clone();
            let recv_v = rooted_values[2].clone();
            let rooted_result = ctx.block().call(
                DOUBLE,
                "js_object_super_get",
                &[(DOUBLE, &home_v), (DOUBLE, &key_v), (DOUBLE, &recv_v)],
            );
            rooted_group.release(ctx);
            Ok(rooted_result)
        }

        Expr::ObjectSuperPropertySet {
            home,
            key,
            value,
            receiver,
        } => {
            crate::rooting::with_rooted_group(ctx, 6, |ctx, group| {
                let h = group.lower(ctx, home, true)?;
                let k = group.lower(ctx, key, true)?;
                let r = group.lower(ctx, receiver, true)?;
                // The reference captures its home, raw key, and receiver before
                // evaluating the RHS. Keep the raw key: PutValue applies
                // ToPropertyKey only after the RHS and the live home parent
                // lookup, inside js_put_value_set's rooted runtime path.
                let v = group.lower(ctx, value, true)?;
                let home_value = group.reread(ctx, h)?;
                let base = ctx.block().call(
                    DOUBLE,
                    "js_object_get_prototype_of",
                    &[(DOUBLE, &home_value)],
                );
                let b = group.adopt_emitted(ctx, crate::rooting::Repr::Boxed, &base, true);
                let base = group.reread_emitted(ctx, b);
                let key_value = group.reread(ctx, k)?;
                let receiver = group.reread(ctx, r)?;
                let value = group.reread(ctx, v)?;
                Ok(ctx.block().call(
                    DOUBLE,
                    "js_put_value_set",
                    &[
                        (DOUBLE, &base),
                        (DOUBLE, &key_value),
                        (DOUBLE, &value),
                        (DOUBLE, &receiver),
                        (I32, "1"),
                    ],
                ))
            })
        }

        Expr::ObjectSuperMethodCall {
            home,
            key,
            receiver,
            args,
        } => {
            // An instance method's own home binding: the class's prototype.
            if let (Expr::LocalGet(id), Expr::String(method), Expr::This) =
                (home.as_ref(), key.as_ref(), receiver.as_ref())
            {
                if ctx.method_home_local.map(|(h, _)| h) == Some(*id) {
                    match super_target(ctx, method) {
                        Ok(target) => return lower_proven_super_call(ctx, method, args, target),
                        Err(reason) => record_super_site(ctx, method, "super_generic", reason),
                    }
                }
            }
            crate::rooting::with_rooted_group(ctx, args.len() + 4, |ctx, group| {
                let h = group.lower(ctx, home, true)?;
                let k = group.lower(ctx, key, true)?;
                let r = group.lower(ctx, receiver, true)?;
                let home_value = group.reread(ctx, h)?;
                let key_value = group.reread(ctx, k)?;
                let recv = group.reread(ctx, r)?;
                // Get the callable before argument expressions can observe it.
                let callable = ctx.block().call(
                    DOUBLE,
                    "js_object_super_get",
                    &[(DOUBLE, &home_value), (DOUBLE, &key_value), (DOUBLE, &recv)],
                );
                let f = group.adopt_emitted(ctx, crate::rooting::Repr::Boxed, &callable, true);
                let mut ids = Vec::new();
                for arg in args {
                    ids.push(group.lower(ctx, arg, true)?);
                }
                let mut values = Vec::new();
                for id in ids {
                    values.push(group.reread(ctx, id)?);
                }
                let (ptr, len) = crate::lower_call::emit_method_args_buffer(ctx, &values);
                let callable = group.reread_emitted(ctx, f);
                let recv = group.reread(ctx, r)?;
                Ok(ctx.block().call(
                    DOUBLE,
                    "js_method_call_value",
                    &[
                        (DOUBLE, &callable),
                        (DOUBLE, &recv),
                        (PTR, &ptr),
                        (I64, &len),
                    ],
                ))
            })
        }

        // -------- fs.readFileSync(path) -> Buffer (no encoding) --------
        // Node returns a Buffer when no encoding is supplied; mirror that.
        // js_fs_read_file_binary returns a raw *mut BufferHeader registered
        // in BUFFER_REGISTRY; NaN-box with POINTER_TAG so downstream
        // console.log / .toString / .length / .[i] dispatch consult the
        // registry and format the value as `<Buffer xx xx ...>` (or the
        // appropriate Buffer behaviour for each method).
        Expr::FsReadFileBinary(path) => {
            let path_box = lower_expr(ctx, path)?;
            let blk = ctx.block();
            let buf_handle = blk.call(I64, "js_fs_read_file_binary", &[(DOUBLE, &path_box)]);
            Ok(nanbox_pointer_inline(blk, &buf_handle))
        }

        // -------- instanceof --------
        // Look up the target class's id and call js_instanceof. The
        // runtime walks the object's class chain and returns a
        // NaN-tagged TAG_TRUE/TAG_FALSE double directly — no
        // conversion needed.
        _ => unreachable!("expr/mod.rs dispatched a variant not handled by this submodule"),
    }
}

/// An instance method body whose home binding (the first `let` that
/// `class_home::finish` inserts) is read only by `super.m(...)` sites this
/// module proves: those sites never read the home value, so the binding is
/// not emitted. `None` when any other read (a generic super site, a `super.x`
/// read, an arrow capturing the home) needs it.
pub(crate) fn elided_method_home(ctx: &FnCtx<'_>, body: &[perry_hir::Stmt]) -> Option<(u32, bool)> {
    let Some(perry_hir::Stmt::Let {
        id,
        mutable: false,
        init:
            Some(Expr::ClassMethodHome {
                class_name,
                is_static: false,
            }),
        ..
    }) = body.first()
    else {
        return None;
    };
    if ctx.in_static_member || ctx.class_stack.last() != Some(class_name) {
        return None;
    }
    let mut unproven = |e: &Expr| home_read_unproven(ctx, e, *id);
    if body[1..]
        .iter()
        .any(|s| perry_hir::walker::stmt_any_expr(s, &mut unproven))
    {
        return None;
    }
    Some((*id, true))
}

/// Does `e` read the home local `home` anywhere but as the home of a proven
/// `super.m(...)`?
fn home_read_unproven(ctx: &FnCtx<'_>, e: &Expr, home: u32) -> bool {
    match e {
        Expr::ObjectSuperMethodCall {
            home: h,
            key,
            receiver,
            args,
        } if matches!(h.as_ref(), Expr::LocalGet(id) if *id == home) => {
            let proven = matches!(
                (key.as_ref(), receiver.as_ref()),
                (Expr::String(m), Expr::This) if super_target(ctx, m).is_ok()
            );
            !proven || args.iter().any(|a| home_read_unproven(ctx, a, home))
        }
        Expr::LocalGet(id) => *id == home,
        Expr::Closure {
            captures,
            mutable_captures,
            ..
        } if captures.contains(&home) || mutable_captures.contains(&home) => true,
        _ => {
            let mut found = false;
            perry_hir::walker::walk_expr_children(e, &mut |c: &Expr| {
                found = found || home_read_unproven(ctx, c, home);
            });
            found
        }
    }
}

/// A `super.m(...)` the compiler proves: the home class's static chain to the
/// ConstFn holder of `m` (`static_prototype::super_chain`), the holder's
/// method body and its declared parameter count.
struct SuperTarget {
    home_cid: u32,
    chain: Vec<(u32, u32)>,
    body: String,
    arity: usize,
}

/// `super.m` in an instance method (or constructor) of a class whose
/// prototype is the static image the compiler built: the home's
/// [[GetPrototypeOf]] and the parent chain's [[Get]] of `m` resolve to the
/// holder's declared method while no prototype on the chain is shadowed.
/// `Err` names why the site keeps the generic get and call.
fn super_target(ctx: &FnCtx<'_>, method: &str) -> std::result::Result<SuperTarget, &'static str> {
    if ctx.in_static_member {
        return Err("static");
    }
    if crate::target_layout::target_is_ilp32(ctx.target_triple) {
        return Err("target");
    }
    let class = ctx.class_stack.last().ok_or("no-class")?;
    let home_cid = *ctx.class_ids.get(class).ok_or("no-class")?;
    if home_cid == 0 || home_cid >= 0x7FFF_FF00 {
        return Err("no-static-image");
    }
    let (chain, holder) = crate::codegen::static_prototype::super_chain(ctx, class, method)
        .ok_or("no-static-chain")?;
    let key = (holder, method.to_string());
    let declaration = ctx
        .classes
        .get(&key.0)
        .and_then(|c| c.methods.iter().find(|m| m.name == method))
        .ok_or("no-static-chain")?;
    if declaration.is_async
        || declaration.is_generator
        || declaration
            .params
            .iter()
            .any(|p| p.is_rest || p.arguments_object.is_some())
        || [
            ctx.method_has_rest,
            ctx.method_has_synthetic_arguments,
            ctx.method_arguments_length_only,
        ]
        .iter()
        .any(|m| m.get(&key).copied().unwrap_or(false))
    {
        return Err("arguments");
    }
    let body = ctx.methods.get(&key).ok_or("no-body")?.clone();
    let arity = *ctx.method_param_counts.get(&key).ok_or("no-body")?;
    Ok(SuperTarget {
        home_cid,
        chain,
        body,
        arity,
    })
}

/// `super.m(args)` in spec order: the receiver (`this`, which throws before
/// `super()`), the [[Get]] of `m`, then the arguments, then the call. The get
/// is the holder proof over the static chain; only where it fails does the
/// cold block read the live home parent (`js_super_accessor_get`). The
/// arguments are lowered once; the proven arm calls the holder's body
/// directly (missing arguments `undefined`, extra ones evaluated and
/// dropped), the other calls the value the get read.
fn lower_proven_super_call(
    ctx: &mut FnCtx<'_>,
    method: &str,
    args: &[Expr],
    target: SuperTarget,
) -> Result<String> {
    let this_expr = Expr::This;
    let collects = crate::rooting::any_operand_may_collect(ctx, args.iter());
    crate::rooting::with_rooted_group(ctx, args.len() + 2, |ctx, group| {
        let r = group.lower(ctx, &this_expr, collects)?;
        // The holder proof reads no user state; compute it before the
        // arguments so a write among them cannot change the callee.
        let ok = crate::proto_constants::chain_constant(ctx, method, &target.chain);
        let how = if ok == "true" { "constant" } else { "guarded" };
        record_super_site(ctx, method, "super_direct", how);
        let f = if ok == "true" {
            None
        } else {
            let slow = ctx.new_block("super.get.slow");
            let done = ctx.new_block("super.get.done");
            let slow_l = ctx.block_label(slow);
            let done_l = ctx.block_label(done);
            let fast_end = ctx.block().label.clone();
            ctx.block().cond_br(&ok, &done_l, &slow_l);
            ctx.current_block = slow;
            let key = lower_expr(ctx, &Expr::String(method.to_string()))?;
            let recv = group.reread(ctx, r)?;
            let got = ctx.block().call(
                DOUBLE,
                "js_super_accessor_get",
                &[
                    (I32, &target.home_cid.to_string()),
                    (DOUBLE, &key),
                    (DOUBLE, &recv),
                ],
            );
            let slow_end = ctx.block().label.clone();
            ctx.block().br(&done_l);
            ctx.current_block = done;
            let undefined =
                crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let f = ctx
                .block()
                .phi(DOUBLE, &[(&undefined, &fast_end), (&got, &slow_end)]);
            Some(group.adopt_emitted(ctx, crate::rooting::Repr::Boxed, &f, collects))
        };
        let mut ids = Vec::with_capacity(args.len());
        for (i, arg) in args.iter().enumerate() {
            let later = crate::rooting::any_operand_may_collect(ctx, args[i + 1..].iter());
            ids.push(group.lower(ctx, arg, later)?);
        }
        let mut values = Vec::with_capacity(ids.len());
        for id in ids {
            values.push(group.reread(ctx, id)?);
        }
        let recv = group.reread(ctx, r)?;
        let direct = |ctx: &mut FnCtx<'_>| {
            let undefined =
                crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let mut call_args = vec![recv.clone()];
            call_args.extend(values.iter().take(target.arity).cloned());
            call_args.resize(target.arity + 1, undefined);
            let slices: Vec<_> = call_args.iter().map(|v| (DOUBLE, v.as_str())).collect();
            ctx.block().call(DOUBLE, &target.body, &slices)
        };
        let Some(f) = f else {
            return Ok(direct(ctx));
        };
        let fast = ctx.new_block("super.direct");
        let generic = ctx.new_block("super.generic");
        let merge = ctx.new_block("super.merge");
        let fast_l = ctx.block_label(fast);
        let generic_l = ctx.block_label(generic);
        let merge_l = ctx.block_label(merge);
        ctx.block().cond_br(&ok, &fast_l, &generic_l);
        ctx.current_block = fast;
        let r1 = direct(ctx);
        let fast_end = ctx.block().label.clone();
        ctx.block().br(&merge_l);
        ctx.current_block = generic;
        let (ptr, len) = crate::lower_call::emit_method_args_buffer(ctx, &values);
        let callable = group.reread_emitted(ctx, f);
        emit_callable_or_throw(ctx, &callable, method);
        let r2 = ctx.block().call(
            DOUBLE,
            "js_method_call_value",
            &[
                (DOUBLE, &callable),
                (DOUBLE, &recv),
                (PTR, &ptr),
                (I64, &len),
            ],
        );
        let generic_end = ctx.block().label.clone();
        ctx.block().br(&merge_l);
        ctx.current_block = merge;
        Ok(ctx
            .block()
            .phi(DOUBLE, &[(&r1, &fast_end), (&r2, &generic_end)]))
    })
}

/// Call(F, ...) of a value `super.m` read: a non-callable throws Node's
/// `TypeError: (intermediate value).m is not a function` (IsCallable is
/// `typeof F === "function"`).
fn emit_callable_or_throw(ctx: &mut FnCtx<'_>, f: &str, method: &str) {
    let function = super::compare::typeof_literal_tag("function")
        .expect("typeof tag of function")
        .to_string();
    let tag = ctx.block().call(I32, "js_value_typeof_tag", &[(DOUBLE, f)]);
    let callable = ctx.block().icmp_eq(I32, &tag, &function);
    let call = ctx.new_block("super.callable");
    let throw = ctx.new_block("super.not_callable");
    let call_l = ctx.block_label(call);
    let throw_l = ctx.block_label(throw);
    ctx.block().cond_br(&callable, &call_l, &throw_l);
    ctx.current_block = throw;
    let label = |ctx: &mut FnCtx<'_>, text: &str| {
        let idx = ctx.strings.intern(text);
        let entry = ctx.strings.entry(idx);
        (
            format!("@{}", entry.bytes_global),
            entry.byte_len.to_string(),
        )
    };
    let (kind, kind_len) = label(ctx, "intermediate value");
    let (name, name_len) = label(ctx, method);
    let blk = ctx.block();
    blk.call_void(
        "js_throw_type_error_not_a_function",
        &[
            (PTR, &kind),
            (I64, &kind_len),
            (PTR, &name),
            (I64, &name_len),
        ],
    );
    blk.unreachable();
    ctx.current_block = call;
}

/// `--explain-lowering`: one record per `super.m()` site, `super_direct`
/// (constant | guarded) or `super_generic` (the reason).
fn record_super_site(ctx: &mut FnCtx<'_>, method: &str, consumer: &str, why: &str) {
    let lowered = crate::native_value::LoweredValue::js_value("undefined");
    ctx.record_lowered_value(
        "SuperMethodCall",
        None,
        consumer,
        &lowered,
        None,
        None,
        None,
        false,
        false,
        vec![
            format!("super_method={method}"),
            format!("super_lowering={why}"),
        ],
    );
}

/// Constructors and static initialization retain their lexical class context;
/// ordinary methods bind this once in HIR, where arrows capture it too.
fn lexical_class_home(ctx: &FnCtx<'_>) -> Result<Expr> {
    let class_name = ctx
        .class_stack
        .last()
        .cloned()
        .ok_or_else(|| anyhow!("super property outside a class body"))?;
    Ok(Expr::ClassMethodHome {
        class_name,
        is_static: ctx.in_static_member,
    })
}

#[cfg(test)]
#[path = "super_method_tests.rs"]
mod tests;
