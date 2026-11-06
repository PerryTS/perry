//! `recv.m(...args)` and `f(...args)`: the plain call's dispatch with a
//! different argument producer (THR2, DESIGN 2b).
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
//!   The source is admitted when nobody can observe its iteration: in a
//!   program with no iteration write, the guarded element read's own guard
//!   inline (an ordinary unforwarded Array whose needed slots lie in its
//!   dense prefix; `guarded_array::emit_spread_stack_admission`), else
//!   `js_array_destructure_needs_iterator(src) == 0`. Admitted, the
//!   parameters come straight from the source: `j < pre ? pre[j] : k < len ? src[k] : post[k - len]`.
//!   No copy, no allocation, no arity cap. A miss joins the vector form.
//! * **vector form**: one argument array built in source order with the one
//!   spread drain (`js_array_spread_append`), then the same selected arm reads
//!   its parameters from it; the generic arm applies the vector.
//!
//! Rest and `arguments` bodies take the generic arm (their direct ABI
//! allocates the bundles; the vector already is one).
//!
//! `f(...args)` on a module function is the same producer with one arm
//! ([`emit_function`]): a fixed-arity body takes the stack or vector form; a
//! rest or `arguments` body takes the vector form, so its arrays are the
//! drain's fresh copy and never the caller's source.

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
            s.cased[i]
                && !has_rest
                && !has_synth
                && !s.implementors[i].1.starts_with("perry_static_")
        })
        .collect()
}

/// The stack form's spread position: exactly one spread operand, and every
/// operand after it order-free.
fn stack_position(ctx: &FnCtx<'_>, args: &[CallArg]) -> Option<usize> {
    let mut spreads = args
        .iter()
        .enumerate()
        .filter(|(_, a)| matches!(a, CallArg::Spread(_)));
    let (k, _) = spreads.next()?;
    if spreads.next().is_some() {
        return None;
    }
    // A class instance whose `[Symbol.iterator]` member the site proves is
    // never an admitted array: its drain is the vector form's.
    if crate::proto_constants::iterator_member_site(ctx, arg_expr(&args[k])).is_some() {
        return None;
    }
    let post: Vec<Expr> = args[k + 1..].iter().map(|a| arg_expr(a).clone()).collect();
    (!crate::expr::method_site::args_may_observe_lookup(ctx, &post)).then_some(k)
}

/// A value its call evaluated before the arguments (the receiver, the method
/// read). The producer keeps it rooted across them and hands it back re-read.
#[derive(Clone, Copy)]
enum Held {
    Root(usize),
    Emitted(rooting::EmittedValue),
}

fn read_held(
    ctx: &mut FnCtx<'_>,
    roots: &rooting::RootedGroup<'_>,
    held: &[Held],
) -> Result<Vec<String>> {
    held.iter()
        .map(|h| match *h {
            Held::Root(i) => roots.reread(ctx, i),
            Held::Emitted(v) => Ok(roots.reread_emitted(ctx, v)),
        })
        .collect()
}

/// The spread argument producer. The operands are evaluated in source order,
/// then:
///
/// * **stack form** (`stack_at`): the source is admitted when nobody can
///   observe its iteration (`need` = the slots the direct consumer reads past
///   the leading operands). Admitted, `direct` gets the parameters' [`Parts`]
///   and returns its arms' phi inputs, each arm ending in a branch to `merge`;
///   its `default` label is the vector form.
/// * **vector form**: one argument array built in source order with the one
///   spread drain, then `vector`. The array is fresh: it is never the source.
fn produce<'f, 'a>(
    ctx: &mut FnCtx<'f>,
    roots: &mut rooting::RootedGroup<'a>,
    held: &[Held],
    args: &'a [CallArg],
    stack_at: Option<usize>,
    need: usize,
    direct: impl FnOnce(
        &mut FnCtx<'f>,
        &[String],
        &Parts<'_>,
        &str,
        &str,
    ) -> Result<Vec<(String, String)>>,
    vector: impl FnOnce(&mut FnCtx<'f>, &[String], &str) -> Result<String>,
) -> Result<String> {
    let Some(k) = stack_at else {
        // Vector form: evaluating the operands in order is the drain.
        let roots = &*roots;
        return crate::expr::call_spread::bundle_args_rooted(ctx, args, false, |ctx, v| {
            let h = read_held(ctx, roots, held)?;
            vector(ctx, &h, v)
        });
    };
    let exprs: Vec<&'a Expr> = args.iter().map(arg_expr).collect();
    let mut idxs = Vec::with_capacity(exprs.len());
    for (i, e) in exprs.iter().enumerate() {
        let collects = rooting::any_operand_may_collect(ctx, exprs[i + 1..].iter().copied());
        idxs.push(roots.lower(ctx, *e, collects)?);
    }
    let held_v = read_held(ctx, roots, held)?;
    let vals: Vec<String> = idxs
        .iter()
        .map(|&i| roots.reread(ctx, i))
        .collect::<Result<_>>()?;
    let src = vals[k].clone();

    let stack_idx = ctx.new_block("sdispatch.stack");
    let vec_idx = ctx.new_block("sdispatch.vector");
    let merge_idx = ctx.new_block("sdispatch.merge");
    let stack_l = ctx.block_label(stack_idx);
    let vec_l = ctx.block_label(vec_idx);
    let merge_l = ctx.block_label(merge_idx);
    // Admission: with no iteration write in the program, the guarded
    // element read's own guard, inline (no call on the admitted path);
    // otherwise the runtime's complete proof.
    let (src_h, len, cap) = if crate::expr::array_iteration_facts(&ctx.wpfacts) == 0
        && !crate::expr::typed_feedback_emission_enabled()
    {
        let (h, l, c) = crate::expr::emit_spread_stack_admission(ctx, &src, need, &vec_l);
        ctx.block().br(&stack_l);
        ctx.current_block = stack_idx;
        (h, l, Some(c))
    } else {
        let needs = ctx.block().call(
            I32,
            "js_array_destructure_needs_iterator",
            &[(DOUBLE, &src)],
        );
        let admitted = ctx.block().icmp_eq(I32, &needs, "0");
        ctx.block().cond_br(&admitted, &stack_l, &vec_l);
        ctx.current_block = stack_idx;
        let h = unbox_to_i64(ctx.block(), &src);
        let l = ctx.block().call(I32, "js_array_length", &[(I64, &h)]);
        (h, l, None)
    };

    // Admitted: the parameters come straight from the source.
    let parts = Parts {
        pre: &vals[..k],
        src: &src_h,
        len: &len,
        cap: cap.as_deref(),
        post: &vals[k + 1..],
    };
    let mut phi = direct(ctx, &held_v, &parts, &vec_l, &merge_l)?;

    // Refused source, or an arm with no direct body: build the vector from
    // the evaluated operands (the drain is the only collecting step, so they
    // are rooted on this branch only).
    ctx.current_block = vec_idx;
    let mut fb = rooting::open_rooted_group(vals.len() + held_v.len());
    let r_held: Vec<_> = held_v
        .iter()
        .map(|v| fb.adopt_emitted(ctx, Repr::Boxed, v, true))
        .collect();
    let r_vals: Vec<_> = vals
        .iter()
        .map(|v| fb.adopt_emitted(ctx, Repr::Boxed, v, true))
        .collect();
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
                if i == k {
                    crate::expr::call_spread::emit_spread_append(ctx, acc, arg_expr(&args[k]), &v);
                } else {
                    acc.advance(ctx, "js_array_push_f64", &[Arg::Plain(DOUBLE, &v)]);
                }
            }
            Ok(())
        },
        |ctx, vec| {
            let h: Vec<String> = r_held.iter().map(|r| fb.reread_emitted(ctx, *r)).collect();
            vector(ctx, &h, vec)
        },
    )?;
    fb.release(ctx);
    let after = ctx.block().label.clone();
    ctx.block().br(&merge_l);
    phi.push((v, after));

    ctx.current_block = merge_idx;
    let incoming: Vec<(&str, &str)> = phi.iter().map(|(v, l)| (v.as_str(), l.as_str())).collect();
    Ok(ctx.block().phi(DOUBLE, &incoming))
}

pub(super) fn emit(ctx: &mut FnCtx<'_>, s: Site<'_>) -> Result<String> {
    let exprs: Vec<&Expr> = s.args.iter().map(arg_expr).collect();
    let stack_at = stack_position(ctx, s.args);
    let mut direct = direct_arms(&s);

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
        let (selected, arms) =
            super::select::select(ctx, &recv, s.property, s.implementors, s.cased);
        // A selection names only an arm's answer implementor; the classes it
        // merged into that arm have no switch case of their own.
        for (i, d) in direct.iter_mut().enumerate() {
            *d &= arms.contains(&i);
        }
        selected
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
    let tower = Tower {
        s: &s,
        direct: &direct,
        selected: &selected,
        claimed: &claimed,
        method_id: &method_id,
        undefined: &undefined,
    };
    let need = stack_at.map_or(0, |k| {
        (0..s.implementors.len())
            .filter(|&i| direct[i])
            .map(|i| s.impl_meta[i].3.saturating_sub(k))
            .max()
            .unwrap_or(0)
    });
    let mut held = vec![Held::Root(recv_idx)];
    held.extend(pre_lookup.map(Held::Emitted));
    let result = produce(
        ctx,
        &mut roots,
        &held,
        s.args,
        stack_at,
        need,
        |ctx, h, parts, default_l, merge_l| {
            tower.emit_direct_switch(ctx, &h[0], parts, default_l, merge_l)
        },
        |ctx, h, vector| tower.emit_vector(ctx, &h[0], vector, h.get(1).map(String::as_str)),
    )?;
    roots.release(ctx);
    Ok(result)
}

/// `f(...args)` where the call names a module function `fname`: the plain
/// call with the same argument producer. `fixed` leading parameters, then a
/// `...rest` array (`rest`) and a synthetic `arguments` array (`arguments`).
///
/// A fixed-arity body reads its parameters from the admitted source or from
/// the vector. A rest or `arguments` body takes the vector form: its arrays
/// are the drain's fresh copy (a slice of it past the fixed parameters), so
/// the callee never holds the caller's source.
pub(crate) fn emit_function(
    ctx: &mut FnCtx<'_>,
    fname: &str,
    args: &[CallArg],
    fixed: usize,
    rest: bool,
    arguments: bool,
) -> Result<String> {
    let undefined = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
    let call = |ctx: &mut FnCtx<'_>, values: &[String]| {
        let slices: Vec<(crate::types::LlvmType, &str)> =
            values.iter().map(|a| (DOUBLE, a.as_str())).collect();
        ctx.block().call(DOUBLE, fname, &slices)
    };
    if !rest && !arguments {
        let stack_at = stack_position(ctx, args);
        let need = stack_at.map_or(0, |k| fixed.saturating_sub(k));
        let mut roots = rooting::open_rooted_group(args.len());
        let result = produce(
            ctx,
            &mut roots,
            &[],
            args,
            stack_at,
            need,
            |ctx, _, parts, _, merge_l| {
                let values = params(ctx, parts, fixed, &undefined);
                let v = call(ctx, &values);
                let after = ctx.block().label.clone();
                ctx.block().br(merge_l);
                Ok(vec![(v, after)])
            },
            |ctx, _, vector| {
                let len = ctx.block().call(I32, "js_array_length", &[(I64, vector)]);
                let parts = Parts {
                    pre: &[],
                    src: vector,
                    len: &len,
                    cap: None,
                    post: &[],
                };
                let values = params(ctx, &parts, fixed, &undefined);
                Ok(call(ctx, &values))
            },
        )?;
        roots.release(ctx);
        return Ok(result);
    }
    // Leading operands that are fixed parameters are passed as themselves;
    // the vector starts at `cut`. `arguments` reflects every argument.
    let first_spread = args
        .iter()
        .position(|a| matches!(a, CallArg::Spread(_)))
        .unwrap_or(args.len());
    let cut = if arguments {
        0
    } else {
        first_spread.min(fixed)
    };
    let mut roots = rooting::open_rooted_group(cut + 1);
    for a in &args[..cut] {
        roots.lower(ctx, arg_expr(a), true)?;
    }
    let result =
        crate::expr::call_spread::bundle_args_rooted(ctx, &args[cut..], false, |ctx, vector| {
            // The rest slice allocates: the vector is re-read below it.
            let slicing = rest && (fixed > cut || arguments);
            let r_vec = roots.adopt_emitted(ctx, Repr::Ptr, vector, slicing);
            let slice = slicing.then(|| {
                let from = (fixed - cut).to_string();
                ctx.block().call(
                    I64,
                    "js_array_slice",
                    &[(I64, vector), (I32, &from), (I32, &i32::MAX.to_string())],
                )
            });
            let vector = roots.reread_emitted(ctx, r_vec);
            let mut values: Vec<String> = (0..cut)
                .map(|i| roots.reread(ctx, i))
                .collect::<Result<_>>()?;
            if fixed > cut {
                let len = ctx.block().call(I32, "js_array_length", &[(I64, &vector)]);
                let parts = Parts {
                    pre: &[],
                    src: &vector,
                    len: &len,
                    cap: None,
                    post: &[],
                };
                values.extend(params(ctx, &parts, fixed - cut, &undefined));
            }
            if rest {
                let h = slice.as_deref().unwrap_or(&vector);
                values.push(crate::expr::nanbox_pointer_inline(ctx.block(), h));
            }
            if arguments {
                let marked =
                    ctx.block()
                        .call(I64, "js_array_mark_arguments_object", &[(I64, &vector)]);
                values.push(crate::expr::nanbox_pointer_inline(ctx.block(), &marked));
            }
            Ok(call(ctx, &values))
        })?;
    roots.release(ctx);
    Ok(result)
}

/// Where parameter `j` of a direct arm comes from.
struct Parts<'p> {
    pre: &'p [String],
    /// `i64` handle of the source array, and its `i32` length.
    src: &'p str,
    len: &'p str,
    /// The capacity of an inline-admitted source: its first `len` needed
    /// slots are dense-prefix slots, loaded directly. `None`: the element
    /// read is the runtime's.
    cap: Option<&'p str>,
    post: &'p [String],
}

/// Parameter `j < declared` of a fixed-arity body:
/// `j < pre ? pre[j] : k < len ? src[k] : post[k - len]`, else `undefined`.
/// The element read is branched, never speculated (an index past the
/// end would consult `Array.prototype`).
fn params(ctx: &mut FnCtx<'_>, p: &Parts<'_>, declared: usize, undefined: &str) -> Vec<String> {
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
        let read = match p.cap {
            Some(cap) => {
                let blk = ctx.block();
                let addr = blk.array_element_addr_with_capacity(p.src, cap, &k);
                let ptr = blk.inttoptr(I64, &addr);
                let bits = blk.load(I64, &ptr);
                blk.bitcast_i64_to_double(&bits)
            }
            None => ctx
                .block()
                .call(DOUBLE, "js_array_get_f64", &[(I64, p.src), (I32, &k)]),
        };
        ctx.block().br(&join_l);
        ctx.current_block = tail_idx;
        let mut tail = undefined.to_string();
        if !p.post.is_empty() {
            let t = ctx.block().sub(I32, &k, p.len);
            for (m, v) in p.post.iter().enumerate().rev() {
                let hit = ctx.block().icmp_eq(I32, &t, &m.to_string());
                tail = ctx.block().select(I1, &hit, DOUBLE, v, &tail);
            }
        }
        ctx.block().br(&join_l);
        ctx.current_block = join_idx;
        out.push(
            ctx.block()
                .phi(DOUBLE, &[(&read, &read_l), (&tail, &tail_l)]),
        );
    }
    out
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
            arms.iter()
                .find(|(j, _)| *j == i)
                .map(|&(_, b)| ctx.block_label(b))
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
            call_args.extend(params(ctx, p, s.impl_meta[i].3, self.undefined));
            let slices: Vec<(crate::types::LlvmType, &str)> =
                call_args.iter().map(|a| (DOUBLE, a.as_str())).collect();
            let v = match tower_pshape_route(
                ctx,
                s.impl_owner[i].as_deref(),
                s.property,
                &fname,
                &[],
            ) {
                Some(route) => {
                    let recv_handle = unbox_to_i64(ctx.block(), recv_box);
                    let owner = s.impl_owner[i].as_deref().unwrap_or_default();
                    let layout_proven =
                        s.proven.is_some() && tower_layout_proven(ctx, owner, s.implementors[i].0);
                    emit_tower_pshape_call(
                        ctx,
                        i,
                        &route,
                        &recv_handle,
                        &slices,
                        layout_proven,
                        owner,
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
        let parts = Parts {
            pre: &[],
            src: vector,
            len: &len,
            cap: None,
            post: &[],
        };
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
        let incoming: Vec<(&str, &str)> =
            phi.iter().map(|(v, l)| (v.as_str(), l.as_str())).collect();
        Ok(ctx.block().phi(DOUBLE, &incoming))
    }
}
