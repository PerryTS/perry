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
