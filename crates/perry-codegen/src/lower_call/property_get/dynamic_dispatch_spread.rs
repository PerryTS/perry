//! `recv.m(...args)`: the plain call's dispatch tower with a different
//! argument producer (THR2, DESIGN 2b).
//!
//! The receiver is evaluated first and the method is **selected before the
//! arguments**, on the same selector the plain call uses (`selected`: the
//! proven class id or the cased-implementor index, `0` = no compiled body).
//! An argument that adds an own method or rewrites the prototype slot cannot
//! change the body this site calls: the verdict is an SSA value taken before
//! any operand runs. Receivers no compiled body claims read their method
//! through the split method site before the arguments, as a plain observing
//! call does.
//!
//! Two producers feed the same selected arm:
//!
//! * **stack form**: exactly one spread operand, in any position, and every
//!   operand after it order-free (it runs no user code, so reading the
//!   source's elements after it is the value the drain would have read).
//!   `js_array_destructure_needs_iterator(src) == 0` (the existing
//!   destructuring admission: an ordinary Array whose iteration nobody can
//!   observe; holes read `undefined`) admits reading the parameters straight
//!   from the source: `j < pre ? pre[j] : k < len ? src[k] : post[k - len]`.
//!   No copy, no allocation, no arity cap. A miss joins the vector form.
//! * **vector form**: one argument array built in source order with the one
//!   spread drain (`js_array_spread_append`), then the same selected arm reads
//!   its parameters from it; the generic arm applies the vector.
//!
//! Rest and `arguments` bodies take the generic arm (their direct ABI
//! allocates the bundles; the vector already is one).

use anyhow::Result;
use perry_hir::{CallArg, Expr};

use crate::expr::{unbox_to_i64, FnCtx};
use crate::nanbox::double_literal;
use crate::rooting::{self, Arg, Repr};
use crate::types::{DOUBLE, I1, I32, I64};

use super::tower::{emit_tower_pshape_call, tower_layout_proven, tower_pshape_route};

pub(super) struct Site<'a> {
    pub object: &'a Expr,
    pub property: &'a str,
    pub args: &'a [CallArg],
    pub implementors: &'a [(u32, String)],
    /// `(has_rest, has_synthetic_arguments, has_user_rest, declared_count)`.
    pub impl_meta: &'a [(bool, bool, bool, usize)],
    pub impl_owner: &'a [Option<String>],
    pub cased: &'a [bool],
    pub proven: Option<&'a (Vec<(u32, usize)>, bool)>,
    pub split: bool,
    pub call_byte_offset: u32,
}

fn arg_expr(a: &CallArg) -> &Expr {
    match a {
        CallArg::Expr(e) | CallArg::Spread(e) => e,
    }
}

/// The selected arms this site calls directly: cased, fixed-arity bodies.
fn direct_arms(s: &Site<'_>) -> Vec<bool> {
    (0..s.implementors.len())
        .map(|i| {
            let (has_rest, has_synth, _, _) = s.impl_meta[i];
            s.cased[i] && !has_rest && !has_synth && !s.implementors[i].1.starts_with("perry_static_")
        })
        .collect()
}

pub(super) fn emit(ctx: &mut FnCtx<'_>, s: Site<'_>) -> Result<String> {
    let exprs: Vec<&Expr> = s.args.iter().map(arg_expr).collect();
    let spreads: Vec<usize> = s
        .args
        .iter()
        .enumerate()
        .filter(|(_, a)| matches!(a, CallArg::Spread(_)))
        .map(|(i, _)| i)
        .collect();
    let stack_at = (spreads.len() == 1).then(|| spreads[0]).filter(|&k| {
        let post: Vec<Expr> = exprs[k + 1..].iter().map(|e| (*e).clone()).collect();
        !crate::expr::method_site::args_may_observe_lookup(ctx, &post)
    });
    let direct = direct_arms(&s);

    let mut roots = rooting::open_rooted_group(1 + exprs.len() + usize::from(s.split));
    // The vector form's operand evaluation is the drain, which allocates.
    let recv_collects = s.split
        || stack_at.is_none()
        || rooting::any_operand_may_collect(ctx, exprs.iter().copied());
    let recv_idx = roots.lower(ctx, s.object, recv_collects)?;
    let recv = roots.reread(ctx, recv_idx)?;
    let key_idx = ctx.strings.intern(s.property);
    let dispatch_global = ctx.strings.static_dispatch_global(key_idx);
    let method_id = crate::strings::emit_static_dispatch_id(ctx.block(), &dispatch_global);
    let selected = if let Some((_, nullable)) = s.proven {
        crate::expr::wpfacts_access::emit_class_id(ctx, &recv, *nullable)
    } else if s.cased.contains(&true) {
        super::select::select(ctx, &recv, s.property, s.implementors, s.cased)
    } else {
        "0".to_string()
    };
    let claimed = ctx.block().icmp_ne(I32, &selected, "0");
    let pre_lookup = if s.split {
        crate::expr::calls::emit_call_location_at(ctx, s.call_byte_offset);
        let lookup = crate::expr::method_site::emit_method_site_lookup_unless(
            ctx, s.property, &recv, &method_id, &claimed,
        );
        let protect =
            stack_at.is_none() || rooting::any_operand_may_collect(ctx, exprs.iter().copied());
        Some(roots.adopt_emitted(ctx, Repr::Boxed, &lookup.value, protect))
    } else {
        None
    };
    let undefined = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
    let tower = Tower { s: &s, direct: &direct, selected: &selected, claimed: &claimed, method_id: &method_id, undefined: &undefined };

    let result = if let Some(k) = stack_at {
        let mut idxs = Vec::with_capacity(exprs.len());
        for (i, e) in exprs.iter().enumerate() {
            let collects = rooting::any_operand_may_collect(ctx, exprs[i + 1..].iter().copied());
            idxs.push(roots.lower(ctx, e, collects)?);
        }
        let recv_box = roots.reread(ctx, recv_idx)?;
        let vals: Vec<String> = idxs.iter().map(|&i| roots.reread(ctx, i)).collect::<Result<_>>()?;
        let pre_value = pre_lookup.map(|v| roots.reread_emitted(ctx, v));
        let src = vals[k].clone();

        let stack_idx = ctx.new_block("sdispatch.stack");
        let vec_idx = ctx.new_block("sdispatch.vector");
        let merge_idx = ctx.new_block("sdispatch.merge");
        let stack_l = ctx.block_label(stack_idx);
        let vec_l = ctx.block_label(vec_idx);
        let merge_l = ctx.block_label(merge_idx);
        let needs = ctx.block().call(I32, "js_array_destructure_needs_iterator", &[(DOUBLE, &src)]);
        let admitted = ctx.block().icmp_eq(I32, &needs, "0");
        ctx.block().cond_br(&admitted, &stack_l, &vec_l);

        // Admitted: the parameters come straight from the source.
        ctx.current_block = stack_idx;
        let src_h = unbox_to_i64(ctx.block(), &src);
        let len = ctx.block().call(I32, "js_array_length", &[(I64, &src_h)]);
        let parts = Parts { pre: &vals[..k], src: &src_h, len: &len, post: &vals[k + 1..] };
        let mut phi = tower.emit_direct_switch(ctx, &recv_box, &parts, &vec_l, &merge_l)?;

        // Refused source, or an arm with no direct body: build the vector from
        // the evaluated operands (the drain is the only collecting step, so
        // they are rooted on this branch only).
        ctx.current_block = vec_idx;
        let mut fb = rooting::open_rooted_group(vals.len() + 2);
        let r_recv = fb.adopt_emitted(ctx, Repr::Boxed, &recv_box, true);
        let r_vals: Vec<_> = vals.iter().map(|v| fb.adopt_emitted(ctx, Repr::Boxed, v, true)).collect();
        let r_pre = pre_value.as_ref().map(|v| fb.adopt_emitted(ctx, Repr::Boxed, v, true));
        let cap = vals.len().to_string();
        let acc0 = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
        let v = rooting::with_rooted_accumulator(
            ctx,
            Repr::Ptr,
            &acc0,
            true,
            |ctx, acc| {
                for (i, r) in r_vals.iter().enumerate() {
                    let v = fb.reread_emitted(ctx, *r);
                    let f = if i == k { "js_array_spread_append" } else { "js_array_push_f64" };
                    acc.advance(ctx, f, &[Arg::Plain(DOUBLE, &v)]);
                }
                Ok(())
            },
            |ctx, vector| {
                let recv = fb.reread_emitted(ctx, r_recv);
                let pre = r_pre.map(|r| fb.reread_emitted(ctx, r));
                tower.emit_vector(ctx, &recv, vector, pre.as_deref())
            },
        )?;
        fb.release(ctx);
        let after = ctx.block().label.clone();
        ctx.block().br(&merge_l);
        phi.push((v, after));

        ctx.current_block = merge_idx;
        let incoming: Vec<(&str, &str)> = phi.iter().map(|(v, l)| (v.as_str(), l.as_str())).collect();
        ctx.block().phi(DOUBLE, &incoming)
    } else {
        // Vector form: evaluating the operands in order is the drain.
        crate::expr::call_spread::bundle_args_rooted(ctx, s.args, false, |ctx, vector| {
            let recv = roots.reread(ctx, recv_idx)?;
            let pre = pre_lookup.map(|v| roots.reread_emitted(ctx, v));
            tower.emit_vector(ctx, &recv, vector, pre.as_deref())
        })?
    };
    roots.release(ctx);
    Ok(result)
}

/// Where parameter `j` of a direct arm comes from.
struct Parts<'p> {
    pre: &'p [String],
    /// `i64` handle of the source array, and its `i32` length.
    src: &'p str,
    len: &'p str,
    post: &'p [String],
}

struct Tower<'t, 'a> {
    s: &'t Site<'a>,
    direct: &'t [bool],
    selected: &'t str,
    claimed: &'t str,
    method_id: &'t str,
    undefined: &'t str,
}

impl Tower<'_, '_> {
    /// Parameter `j < declared` of a fixed-arity body:
    /// `j < pre ? pre[j] : k < len ? src[k] : post[k - len]`, else `undefined`.
    /// The element read is branched, never speculated (an index past the
    /// end would consult `Array.prototype`).
    fn params(&self, ctx: &mut FnCtx<'_>, p: &Parts<'_>, declared: usize) -> Vec<String> {
        let mut out = Vec::with_capacity(declared);
        for j in 0..declared {
            if j < p.pre.len() {
                out.push(p.pre[j].clone());
                continue;
            }
            let k = (j - p.pre.len()).to_string();
            let read_idx = ctx.new_block("sdispatch.arg_read");
            let tail_idx = ctx.new_block("sdispatch.arg_tail");
            let join_idx = ctx.new_block("sdispatch.arg_join");
            let read_l = ctx.block_label(read_idx);
            let tail_l = ctx.block_label(tail_idx);
            let join_l = ctx.block_label(join_idx);
            let inb = ctx.block().icmp_ult(I32, &k, p.len);
            ctx.block().cond_br(&inb, &read_l, &tail_l);
            ctx.current_block = read_idx;
            let read = ctx.block().call(DOUBLE, "js_array_get_f64", &[(I64, p.src), (I32, &k)]);
            ctx.block().br(&join_l);
            ctx.current_block = tail_idx;
            let mut tail = self.undefined.to_string();
            if !p.post.is_empty() {
                let t = ctx.block().sub(I32, &k, p.len);
                for (m, v) in p.post.iter().enumerate().rev() {
                    let hit = ctx.block().icmp_eq(I32, &t, &m.to_string());
                    tail = ctx.block().select(I1, &hit, DOUBLE, v, &tail);
                }
            }
            ctx.block().br(&join_l);
            ctx.current_block = join_idx;
            out.push(ctx.block().phi(DOUBLE, &[(&read, &read_l), (&tail, &tail_l)]));
        }
        out
    }

    /// `switch selected` over the direct arms; every other value goes to
    /// `default_l`. Returns the arms' `(value, block)` phi inputs, each arm
    /// ending in a branch to `merge_l`.
    fn emit_direct_switch(
        &self,
        ctx: &mut FnCtx<'_>,
        recv_box: &str,
        p: &Parts<'_>,
        default_l: &str,
        merge_l: &str,
    ) -> Result<Vec<(String, String)>> {
        let s = self.s;
        let arms: Vec<(usize, usize)> = (0..s.implementors.len())
            .filter(|&i| self.direct[i])
            .map(|i| (i, ctx.new_block(&format!("sdispatch.case{i}"))))
            .collect();
        let label_of = |ctx: &FnCtx<'_>, i: usize| {
            arms.iter().find(|(j, _)| *j == i).map(|&(_, b)| ctx.block_label(b))
        };
        let mut sw = format!("switch i32 {}, label %{default_l} [", self.selected);
        match s.proven {
            Some((map, _)) => {
                for &(cid, i) in map {
                    if let Some(l) = label_of(ctx, i) {
                        sw.push_str(&format!(" i32 {cid}, label %{l}"));
                    }
                }
            }
            None => {
                for &(i, b) in &arms {
                    sw.push_str(&format!(" i32 {}, label %{}", i + 1, ctx.block_label(b)));
                }
            }
        }
        sw.push_str(" ]");
        ctx.block().emit_raw(sw);
        ctx.block().mark_terminated();
        let mut phi = Vec::new();
        for &(i, b) in &arms {
            ctx.current_block = b;
            let fname = s.implementors[i].1.clone();
            let mut call_args = vec![recv_box.to_string()];
            call_args.extend(self.params(ctx, p, s.impl_meta[i].3));
            let slices: Vec<(crate::types::LlvmType, &str)> =
                call_args.iter().map(|a| (DOUBLE, a.as_str())).collect();
            let v = match tower_pshape_route(ctx, s.impl_owner[i].as_deref(), s.property, &fname, &[]) {
                Some(route) => {
                    let recv_handle = unbox_to_i64(ctx.block(), recv_box);
                    let owner = s.impl_owner[i].as_deref().unwrap_or_default();
                    let layout_proven =
                        s.proven.is_some() && tower_layout_proven(ctx, owner, s.implementors[i].0);
                    emit_tower_pshape_call(
                        ctx, i, &route, &recv_handle, &slices, layout_proven, owner,
                        s.implementors[i].0,
                    )
                }
                None => ctx.block().call(DOUBLE, &fname, &slices),
            };
            let after = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(merge_l);
            }
            phi.push((v, after));
        }
        Ok(phi)
    }

    /// The selected arm over a finished argument vector (`i64`); the generic
    /// arm applies it: by id, or the method read before the arguments.
    fn emit_vector(
        &self,
        ctx: &mut FnCtx<'_>,
        recv_box: &str,
        vector: &str,
        pre_value: Option<&str>,
    ) -> Result<String> {
        let generic_idx = ctx.new_block("sdispatch.generic");
        let merge_idx = ctx.new_block("sdispatch.vmerge");
        let generic_l = ctx.block_label(generic_idx);
        let merge_l = ctx.block_label(merge_idx);
        let len = ctx.block().call(I32, "js_array_length", &[(I64, vector)]);
        let parts = Parts { pre: &[], src: vector, len: &len, post: &[] };
        let mut phi = self.emit_direct_switch(ctx, recv_box, &parts, &generic_l, &merge_l)?;
        ctx.current_block = generic_idx;
        let by_id = |ctx: &mut FnCtx<'_>| {
            ctx.block().call(
                DOUBLE,
                "js_native_call_method_apply_by_id",
                &[(DOUBLE, recv_box), (I64, self.method_id), (I64, vector)],
            )
        };
        let v = match pre_value {
            None => by_id(ctx),
            Some(value) => {
                // A claimed receiver's read was skipped (its body is a proven
                // prototype constant); a by-name answer is not a JS value.
                let bits = ctx.block().bitcast_double_to_i64(value);
                let either = ctx.block().or(I64, &bits, "2");
                let named = ctx.block().icmp_eq(
                    I64,
                    &either,
                    &crate::runtime_abi::METHOD_SITE_BY_NAME_DIRECT.to_string(),
                );
                let use_id = ctx.block().or(I1, &named, self.claimed);
                let id_idx = ctx.new_block("sdispatch.by_id");
                let val_idx = ctx.new_block("sdispatch.by_value");
                let join_idx = ctx.new_block("sdispatch.generic_join");
                let id_l = ctx.block_label(id_idx);
                let val_l = ctx.block_label(val_idx);
                let join_l = ctx.block_label(join_idx);
                ctx.block().cond_br(&use_id, &id_l, &val_l);
                ctx.current_block = id_idx;
                let a = by_id(ctx);
                let a_end = ctx.block().label.clone();
                ctx.block().br(&join_l);
                ctx.current_block = val_idx;
                let b = ctx.block().call(
                    DOUBLE,
                    "js_method_call_value_apply",
                    &[(DOUBLE, value), (DOUBLE, recv_box), (I64, vector)],
                );
                let b_end = ctx.block().label.clone();
                ctx.block().br(&join_l);
                ctx.current_block = join_idx;
                ctx.block().phi(DOUBLE, &[(&a, &a_end), (&b, &b_end)])
            }
        };
        let after = ctx.block().label.clone();
        ctx.block().br(&merge_l);
        phi.push((v, after));
        ctx.current_block = merge_idx;
        let incoming: Vec<(&str, &str)> = phi.iter().map(|(v, l)| (v.as_str(), l.as_str())).collect();
        Ok(ctx.block().phi(DOUBLE, &incoming))
    }
}
