//! Class instance method dispatch — the interface/dynamic dispatch tower and
//! the static-fallback + virtual-override tower.
//! Pure code move from `property_get.rs` — no behavior change.

use super::*;

use anyhow::Result;
use perry_hir::Expr;

use crate::expr::{nanbox_pointer_inline, unbox_to_i64, FnCtx};
use crate::nanbox::double_literal;
use crate::type_analysis::receiver_class_name;
use crate::types::{DOUBLE, I32, I64};

#[path = "dynamic_dispatch_collapse.rs"]
mod collapse;
#[path = "dispatch_receiver_class.rs"]
mod dispatch_receiver_class;
#[path = "dynamic_dispatch_select.rs"]
mod select;
#[path = "dynamic_dispatch_tower.rs"]
mod tower;
use collapse::method_dispatch_collapse_enabled;
use dispatch_receiver_class::*;
use tower::{emit_tower_pshape_call, tower_pshape_route};

// Reach the override-emit helpers (`pub(super)` of `lower_call`) by their
// canonical crate-relative path.
use crate::lower_call::method_override::{
    emit_guarded_direct_method_call, emit_pshape_argument_dispatch, SubclassDispatchArm,
};

/// Cap on the number of extra `(class id, keys token)` arms a shape-guarded
/// direct call may carry. A wide hierarchy would turn every callsite into a
/// long inline compare chain — more instruction cache than the single tower
/// call it replaces — so past this width the site keeps the single-arm guard.
const MAX_SUBCLASS_DISPATCH_ARMS: usize = 8;

/// #7142: the proven-`this` clone a class-id dispatch-tower case may route to,
/// plus the keys token the routed path must re-check inline.
struct TowerPshapeRoute {
    /// The `{public}$pshape` clone symbol (same `(double this, args…)` ABI as
    /// the public one — only the body's `this.field` lowering differs).
    clone_fn: String,
    /// Receiver-safe target for the shape-miss arm. When every indexed
    /// parameter is already proved nonnegative i32 at this call site, this is
    /// the unshaped `$idx_u31` clone; otherwise it is the public method.
    generic_fn: String,
    /// `@perry_class_keys_<mod>__<Class>`, holding the class's canonical
    /// keys-array pointer. A receiver still carrying it has the declared packed
    /// layout; `delete` swaps in a freshly cloned array, which is exactly what
    /// the inline compare catches.
    keys_global: String,
}

/// Build the exact direct-call ABI for one concrete method implementation.
/// Virtual towers cannot share this vector: sibling overrides may disagree on
/// declared arity, user rest, or the compiler-synthesized `arguments` slot.
///
/// #8162: `has_synthetic_arguments` and `has_rest` alone cannot size the tail.
/// A body with BOTH a user `...rest` and an `arguments` read declares
/// `[a, rest, arguments]` — TWO trailing array slots, bundled from different
/// offsets over the same argument list. `has_user_rest` (false for a class
/// this module has no HIR for, which keeps the one-slot shape those calls
/// already had) is the bit that tells the two-slot case from synth-only.
fn build_direct_method_args(
    ctx: &mut FnCtx<'_>,
    recv_box: &str,
    user_args: &[String],
    has_rest: bool,
    has_synthetic_arguments: bool,
    has_user_rest: bool,
    arguments_length_only: bool,
    declared_count: usize,
    undefined_lit: &str,
) -> Vec<String> {
    let mut direct_args = Vec::with_capacity(declared_count + 1);
    direct_args.push(recv_box.to_string());
    if has_synthetic_arguments || has_rest {
        let trailing_slots = if has_synthetic_arguments && has_user_rest {
            2
        } else {
            1
        };
        let fixed_user = declared_count.saturating_sub(trailing_slots);
        for index in 0..fixed_user {
            direct_args.push(
                user_args
                    .get(index)
                    .cloned()
                    .unwrap_or_else(|| undefined_lit.to_string()),
            );
        }
        // (first bundled index, mark as arguments object), in callee param
        // order: the user rest slot first (bundling from `fixed_user`), then
        // the synthesized `arguments` slot (from 0 — it must reflect EVERY
        // passed argument, and gets the marking without which the callee's
        // `arguments` is an ordinary Array). A lone trailing slot is the rest
        // shape unless the method synthesizes `arguments`.
        let mut bundles: Vec<(usize, bool)> = Vec::new();
        if has_user_rest || !has_synthetic_arguments {
            bundles.push((fixed_user, false));
        }
        if has_synthetic_arguments && arguments_length_only {
            debug_assert!(!has_user_rest);
            direct_args.push(double_literal(user_args.len() as f64));
        } else if has_synthetic_arguments {
            bundles.push((0, true));
        }
        for (from, mark) in bundles {
            let count = user_args.len().saturating_sub(from);
            let capacity = (count as u32).to_string();
            let mut bundle = ctx.block().call(I64, "js_array_alloc", &[(I32, &capacity)]);
            for value in user_args.iter().skip(from) {
                let block = ctx.block();
                bundle = block.call(I64, "js_array_push_f64", &[(I64, &bundle), (DOUBLE, value)]);
            }
            if mark {
                let block = ctx.block();
                bundle = block.call(I64, "js_array_mark_arguments_object", &[(I64, &bundle)]);
            }
            direct_args.push(nanbox_pointer_inline(ctx.block(), &bundle));
        }
    } else {
        direct_args.extend(user_args.iter().cloned());
        while direct_args.len() < declared_count + 1 {
            direct_args.push(undefined_lit.to_string());
        }
    }
    direct_args
}

/// Every class whose `extends` chain resolves `property`: each class the
/// method table registers it under, and every transitive subclass of one.
/// Unordered and possibly repeating; callers sort and dedup by their own key.
fn classes_resolving<'c>(ctx: &FnCtx<'c>, property: &str) -> Vec<&'c str> {
    let hierarchy = ctx.class_hierarchy;
    let mut out: Vec<&'c str> = Vec::new();
    for definer in hierarchy.method_definers(property) {
        out.push(definer.as_str());
        out.extend(hierarchy.descendants(definer, None));
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// Interface / dynamic dispatch fallback: when the static class is unknown OR
/// resolves to an interface name not in the class registry, BUT the property
/// name corresponds to a method defined on at least one class in the registry,
/// emit a switch on class_id over all classes that have that method. Then the
/// static-fallback + virtual-override tower for typed-instance receivers.
///
/// `call_byte_offset` is this call's captured source offset (for the #5247
/// `js_set_call_location` emission before the runtime dispatch fallback).
pub(crate) fn try_lower_instance_method_call(
    ctx: &mut FnCtx<'_>,
    object: &Expr,
    property: &str,
    args: &[Expr],
    call_byte_offset: u32,
) -> Result<Option<String>> {
    // Runtime-derived evidence may identify the receiver directly. A declared
    // local class is weaker: it may only select the later direct-method guard,
    // whose miss path preserves dynamic JavaScript dispatch.
    let receiver_class = receiver_class_name(ctx, object)
        .or_else(|| guarded_declared_receiver_class_candidate(ctx, object, property, args));
    // Globals use their builtin dispatch even when a class shares the name.
    let is_global = matches!(object, Expr::GlobalGet(_));
    // If the receiver's static type is a well-known built-in with its own
    // runtime method family (Buffer byte readers, Array, Map, Set, …),
    // don't enter the user-class dispatch tower. Otherwise an imported
    // user class that happens to declare the same method name (e.g. a
    // BufferCursor with `readUInt8`) would be enumerated as an
    // implementor and `buf.readUInt8(i)` would fall through to the
    // default 0.0 case when the Buffer's class id doesn't match any
    // tower entry.
    let is_builtin_receiver = match receiver_class.as_deref() {
        Some(name) => matches!(
            name,
            "Buffer"
                | "Uint8Array"
                | "Uint8ClampedArray"
                | "Int8Array"
                | "Int16Array"
                | "Uint16Array"
                | "Int32Array"
                | "Uint32Array"
                | "Float16Array"
                | "Float32Array"
                | "Float64Array"
                | "BigInt64Array"
                | "BigUint64Array"
                | "Array"
                | "ReadonlyArray"
                | "Map"
                | "ReadonlyMap"
                | "Set"
                | "ReadonlySet"
                | "WeakMap"
                | "WeakSet"
                | "Promise"
                | "RegExp"
                | "Date"
        ),
        None => false,
    };
    let needs_dynamic_dispatch = !is_global
        && !is_builtin_receiver
        && match receiver_class.as_deref() {
            None => true,
            Some(name) => !ctx.classes.contains_key(name),
        };
    if !is_global
        && !is_builtin_receiver
        && generator_receiver_candidate(ctx, receiver_class.as_deref(), property)
    {
        return crate::lower_call::lower_split_method_call(
            ctx,
            object,
            property,
            args,
            call_byte_offset,
            &[],
            None,
            None,
        )
        .map(Some);
    }
    // #11910: arguments that can observe when the method is read. The
    // tower's implementor arms read nothing (a proven class id under intact
    // prototype guards names its body); every other receiver must have its
    // method read before the arguments, through the split method site.
    let split = needs_dynamic_dispatch
        && crate::expr::method_site::args_may_observe_lookup(ctx, args)
        && crate::expr::method_site::method_site_enabled(ctx, property, args.len());
    if needs_dynamic_dispatch {
        // Find all (class_id → fn_name) for `property` — including
        // INHERITED methods. Per JS spec, `subInstance.method()` for a
        // method defined on a parent dispatches to the parent's
        // implementation. perry's previous walk only added classes that
        // DIRECTLY declared `property`; subclasses that inherited the
        // method weren't represented in the dispatch tower, so the
        // icmp_eq vs class_id missed and the call fell through to the
        // runtime's js_native_call_method fallback (which returns an
        // empty object for unknown receiver class+method combos).
        // Refs #420 — drizzle's `serial("id").primaryKey()` where
        // primaryKey is on ColumnBuilder (grandparent) but the
        // receiver is a PgSerialBuilder (grandchild).
        //
        // Algorithm: walk every class C in `class_ids`. For each, walk
        // C's parent chain and find the FIRST class that has `property`
        // in `ctx.methods`. Register (C's id → that ancestor's fn_name).
        let mut implementors: Vec<(u32, String)> = Vec::new();
        // #5437: (has_rest, has_synthetic_arguments, has_user_rest,
        // decl_param_count) per implementor, aligned 1:1 with `implementors`,
        // so each case block can build its own per-arity args without
        // rescanning `ctx.methods`. The user-rest bit is #8162's: with both a
        // user `...rest` and a synthesized `arguments` the tail is TWO array
        // slots, which the first two bits alone cannot express.
        let mut impl_meta: Vec<(bool, bool, bool, usize)> = Vec::new();
        // #7142: aligned 1:1 with `implementors` — `Some(class)` exactly when
        // the receiver class of this case DECLARES `property` itself (the walk
        // stopped at its own entry). That is the condition a proven-`this`
        // clone needs: the class the clone was compiled for is then the
        // receiver's exact dynamic class, so `this` cannot be a subclass
        // instance with a longer chain. Inherited dispatch gets `None` and
        // keeps today's lowering.
        let mut impl_owner: Vec<Option<String>> = Vec::new();
        // The receiver class of each implementor entry (inherited ones too).
        let mut impl_class: Vec<String> = Vec::new();
        let mut seen_pairs: std::collections::HashSet<(u32, String)> =
            std::collections::HashSet::new();
        // Walk `class_ids` in a FIXED order, not `HashMap` order (#7622). Each
        // surviving entry becomes one `icmp`-guarded case block in the tower
        // below, so the map's per-process iteration order was the tower's arm
        // order: the same source compiled twice by the same binary emitted the
        // same arms naming DIFFERENT `perry_method_*` callees per position.
        // `seen_pairs` also dedups on `(class_id, fname)`, so with two names
        // sharing a class id (a class-expression self-binding alias, or an
        // imported class registered under both its own and its local-alias
        // name) which arm is emitted FIRST is what the runtime's first-match
        // tower actually executes — order-dependence that is behavioural, not
        // just cosmetic. Sorting by `(class_id, name)` is total and stable.
        //
        // Only a class whose chain reaches a class declaring `property` can
        // produce an entry, so the walk starts from those: every declaring
        // class and everything below it, never the whole class table.
        let mut dispatch_roots: Vec<(&str, u32)> = classes_resolving(ctx, property)
            .into_iter()
            .filter_map(|name| ctx.class_ids.get(name).map(|&id| (name, id)))
            .collect();
        dispatch_roots.sort_unstable_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(b.0)));
        for (start_cls, start_cid) in dispatch_roots {
            let mut cur: Option<String> = Some(start_cls.to_string());
            while let Some(c) = cur {
                let key = (c.clone(), property.to_string());
                if let Some(fname) = ctx.methods.get(&key).cloned() {
                    if seen_pairs.insert((start_cid, fname.clone())) {
                        // `key` is the exact (defining-class, property) where the
                        // method resolved, so its arity metadata is available now.
                        let has_rest = matches!(ctx.method_has_rest.get(&key), Some(&true));
                        let has_synthetic_arguments =
                            matches!(ctx.method_has_synthetic_arguments.get(&key), Some(&true));
                        // `c` is the DEFINING class, so the user-rest bit is
                        // read off the body this case actually calls.
                        let has_user_rest =
                            crate::codegen::arguments::method_has_user_rest(ctx, &c, property);
                        let decl = ctx.method_param_counts.get(&key).copied().unwrap_or(0);
                        impl_owner.push((c == start_cls).then(|| start_cls.to_string()));
                        impl_class.push(start_cls.to_string());
                        implementors.push((start_cid, fname));
                        impl_meta.push((has_rest, has_synthetic_arguments, has_user_rest, decl));
                    }
                    break;
                }
                cur = ctx.classes.get(&c).and_then(|cc| cc.extends_name.clone());
            }
        }
        if !implementors.is_empty() {
            // #9417: the receiver is lowered FIRST — JS evaluation order requires
            // the MemberExpression to be evaluated before the arguments — and
            // consumed LAST, after every argument expression. Each of those is
            // arbitrary user code that can drive an evacuating young-gen minor,
            // and a bare SSA register is not a GC root
            // (`docs/src/internals/gc-rooting-invariant.md`, case 3), so from the
            // first collection onward the receiver names from-space.
            //
            // Nothing faults there. `js_object_get_own_field_or_undef` fails its
            // `obj_type == GC_TYPE_OBJECT` check on the recycled cell and answers
            // TAG_UNDEFINED, the own-override probe misses, and the by-name
            // dispatch runs on a retired address — so the failure surfaces as a
            // wrong answer several steps downstream. That is #9417's `reading
            // 'def'` in the Claude Code bundle: zod's `K4.extend(q, {…})` with the
            // object literal in argument position.
            //
            // `early_branches.rs`'s COMPUTED-key dispatch (`obj[k](…)`) got this
            // in #7210 (3); the named-property tower here did not. Same fix, same
            // combinator: one `RootedGroup` over [receiver, ...args], re-read
            // below the group. `root_reload` then re-derives every later use that
            // a collection point can reach, so the re-read is needed once here
            // rather than at each of the tower's ~dozen consumers.
            //
            // #9480: state the window per operand. The receiver spans every
            // argument; argument i spans only arguments i+1..N. The own-field
            // and class-id probes below are certified GC leaves. Only work
            // emitted after all source arguments (rest/`arguments` bundles or
            // debug call-location bookkeeping) extends every window.
            let collapse_dynamic = crate::codegen::full_outline_ic_enabled()
                && method_dispatch_collapse_enabled()
                && implementors
                    .iter()
                    .all(|(_, f)| !f.starts_with("perry_static_"));
            // The oversized-module outline emits no class-id tower: every
            // receiver takes the universal method site (fused, or split when
            // the arguments can observe the read), whose memo is keyed on the
            // receiver's shape alone. A class instance primes an inherited
            // entry through its class prototype exactly as any other receiver
            // does; a RegExp, a string or a namespace object is never demoted
            // to by-name dispatch because some class declares the same name.
            if collapse_dynamic {
                return Ok(None);
            }
            // A compiled case is a class whose method is a prototype constant
            // (`select::cased`: a chain the compiler names, a slot the write
            // census proves written once). Receivers of any other class keep
            // the universal method site, and a site with no case is that
            // generic arm alone.
            let cased: Vec<bool> = implementors
                .iter()
                .zip(&impl_class)
                .map(|((_, f), class)| select::cased(ctx, class, property, f))
                .collect();
            // FVA: the facts prove the receiver one of its classes, each a
            // cased implementor: the site is the T0set read shape, one load
            // of the receiver's class id and a switch whose arms are the
            // direct calls, with no probe. Its generic arm is only a nullable
            // receiver's (the receiver test a read keeps): a nullish receiver
            // reads its method, before the arguments, and throws.
            let proven: Option<(Vec<(u32, usize)>, bool)> =
                crate::expr::wpfacts_access::proven_receiver_classes(ctx, object).and_then(
                    |(cids, nullable)| {
                        cids.iter()
                            .map(|&cid| {
                                implementors
                                    .iter()
                                    .zip(&cased)
                                    .position(|((c, _), &k)| k && *c == cid)
                                    .map(|i| (cid, i))
                            })
                            .collect::<Option<Vec<_>>>()
                            .map(|m| (m, nullable))
                    },
                );
            let closed = proven.as_ref().is_some_and(|(_, nullable)| !nullable);
            let split = split && !closed;
            let post_args_may_collect = ctx.strings.call_location_for(call_byte_offset).is_some()
                || impl_meta
                    .iter()
                    .any(|&(has_rest, has_synthetic, _, _)| has_rest || has_synthetic);
            let mut roots = crate::rooting::open_rooted_group(1 + args.len() + usize::from(split));
            // The method is selected before the arguments (ECMA-262 13.3.6.1);
            // the receiver's root window is still the arguments' (#9480).
            let recv_collects = split
                || post_args_may_collect
                || crate::rooting::any_operand_may_collect(ctx, args.iter());
            let recv_idx = roots.lower(ctx, object, recv_collects)?;
            let recv = roots.reread(ctx, recv_idx)?;
            let key_idx = ctx.strings.intern(property);
            let dispatch_global = ctx.strings.static_dispatch_global(key_idx);
            let method_id = crate::strings::emit_static_dispatch_id(ctx.block(), &dispatch_global);
            // Object model step 3: the receiver's class id selects the
            // constant body, a branch over the compile-time set of cased
            // classes. 0 = no compiled body: the site's generic arm.
            let selected = if let Some((_, nullable)) = &proven {
                crate::expr::wpfacts_access::emit_class_id(ctx, &recv, *nullable)
            } else if cased.contains(&true) {
                select::select(ctx, &recv, property, &implementors, &cased)
            } else {
                "0".to_string()
            };
            // #11910: a receiver no compiled body claims reads its method
            // through the split method site before the arguments.
            let pre_lookup = if split {
                let claimed = ctx.block().icmp_ne(I32, &selected, "0");
                crate::expr::calls::emit_call_location_at(ctx, call_byte_offset);
                let lookup = crate::expr::method_site::emit_method_site_lookup_unless(
                    ctx, property, &recv, &method_id, &claimed,
                );
                let value = lookup.value.clone();
                let any_arg_collects = crate::rooting::any_operand_may_collect(ctx, args.iter());
                let value_root =
                    roots.adopt_emitted(ctx, crate::rooting::Repr::Boxed, &value, any_arg_collects);
                Some((lookup, value_root))
            } else {
                None
            };
            // #1758 / epic #1785: the raw user args (no `this`, no issue-#235
            // padding, no rest-bundling) drive every concrete callee below.
            let mut arg_idxs: Vec<usize> = Vec::with_capacity(args.len());
            for (index, a) in args.iter().enumerate() {
                let collects = post_args_may_collect
                    || crate::rooting::any_operand_may_collect(ctx, args[index + 1..].iter());
                arg_idxs.push(roots.lower(ctx, a, collects)?);
            }
            let recv_box = roots.reread(ctx, recv_idx)?;
            let mut static_user_args: Vec<String> = Vec::with_capacity(args.len());
            for &idx in &arg_idxs {
                static_user_args.push(roots.reread(ctx, idx)?);
            }
            let undefined_lit = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let generic_idx = ctx.new_block("idispatch.generic");
            let merge_idx = ctx.new_block("idispatch.merge");
            let generic_label = ctx.block_label(generic_idx);
            let merge_label = ctx.block_label(merge_idx);
            // Case `i + 1` is implementor `i`; every miss is the generic arm.
            let case_idxs: Vec<(usize, usize)> = implementors
                .iter()
                .enumerate()
                .filter(|&(i, _)| cased[i])
                .map(|(i, _)| (i, ctx.new_block(&format!("idispatch.case{i}"))))
                .collect();
            let switch = match &proven {
                // The proven classes' ids select their implementors. The last
                // class is the default arm (the receiver is one of them), as
                // the read shape's last slot is; a contract build lists every
                // class and its default is the witness.
                Some((map, nullable)) => {
                    let label_of = |i: usize| {
                        case_idxs
                            .iter()
                            .find(|(j, _)| *j == i)
                            .map(|&(_, idx)| ctx.block_label(idx))
                            .expect("a proven class's implementor is cased")
                    };
                    let contracts = crate::codegen::helpers::check_contracts();
                    let (listed, default) = if *nullable || contracts {
                        (&map[..], generic_label.clone())
                    } else {
                        (&map[..map.len() - 1], label_of(map[map.len() - 1].1))
                    };
                    let mut s = format!("switch i32 {selected}, label %{default} [");
                    for &(cid, i) in listed {
                        s.push_str(&format!(" i32 {cid}, label %{}", label_of(i)));
                    }
                    s.push_str(" ]");
                    s
                }
                None => {
                    let mut s = format!("switch i32 {selected}, label %{generic_label} [");
                    for &(i, idx) in &case_idxs {
                        s.push_str(&format!(" i32 {}, label %{}", i + 1, ctx.block_label(idx)));
                    }
                    s.push_str(" ]");
                    s
                }
            };
            ctx.block().emit_raw(switch);
            ctx.block().mark_terminated();
            let mut phi_inputs: Vec<(String, String)> = Vec::new();
            for &(i, case_idx) in &case_idxs {
                ctx.current_block = case_idx;
                let fname = &implementors[i].1;
                let (has_rest, has_synth, has_user_rest, decl) = impl_meta[i];
                // #5437: this implementor's own declared arity and rest-ness.
                let case_args = build_direct_method_args(
                    ctx,
                    &recv_box,
                    &static_user_args,
                    has_rest,
                    has_synth,
                    has_user_rest,
                    false,
                    decl,
                    &undefined_lit,
                );
                let case_arg_slices: Vec<(crate::types::LlvmType, &str)> =
                    case_args.iter().map(|s| (DOUBLE, s.as_str())).collect();
                let v = match tower_pshape_route(
                    ctx,
                    impl_owner[i].as_deref(),
                    property,
                    fname,
                    args,
                ) {
                    Some(route) => {
                        let recv_handle = unbox_to_i64(ctx.block(), &recv_box);
                        emit_tower_pshape_call(ctx, i, &route, &recv_handle, &case_arg_slices)
                    }
                    None => ctx.block().call(DOUBLE, fname, &case_arg_slices),
                };
                let after = ctx.block().label.clone();
                if !ctx.block().is_terminated() {
                    ctx.block().br(&merge_label);
                }
                phi_inputs.push((v, after));
            }
            // The generic arm: the split call half (the method was read before
            // the arguments), else the universal method site. A proven site
            // has none: its block is the contract build's witness, or dead.
            ctx.current_block = generic_idx;
            if closed {
                if crate::codegen::helpers::check_contracts() {
                    let site = ctx.wpfacts_sites as i64;
                    ctx.wpfacts_sites += 1;
                    let _ = ctx.block().call(
                        I32,
                        "perry_wpfacts_check_call_miss",
                        &[(crate::types::PTR, "null"), (I64, &site.to_string())],
                    );
                }
                ctx.block().unreachable();
            }
            if !closed {
                let v_generic = match pre_lookup {
                    Some((lookup, value_root)) => {
                        let value = roots.reread_emitted(ctx, value_root);
                        crate::lower_call::console_promise::emit_split_call_half(
                            ctx,
                            &lookup,
                            &recv_box,
                            &value,
                            &static_user_args,
                            property,
                            call_byte_offset,
                            &method_id,
                        )
                    }
                    None => crate::lower_call::console_promise::emit_native_method_str_dispatch(
                        ctx,
                        property,
                        call_byte_offset,
                        &recv_box,
                        &static_user_args,
                    ),
                };
                let after_generic = ctx.block().label.clone();
                if !ctx.block().is_terminated() {
                    ctx.block().br(&merge_label);
                }
                phi_inputs.push((v_generic, after_generic));
            }
            ctx.current_block = merge_idx;
            let phi_args: Vec<(&str, &str)> = phi_inputs
                .iter()
                .map(|(v, l)| (v.as_str(), l.as_str()))
                .collect();
            let result = ctx.block().phi(DOUBLE, &phi_args);
            // The release post-dominates every case and the generic arm.
            roots.release(ctx);
            return Ok(Some(result));
        }
    }

    if let Some(class_name) = receiver_class {
        // Step 1: walk parent chain for the static method name.
        let mut static_method: Option<(String, (String, String))> = None;
        let mut current_class = Some(class_name.clone());
        while let Some(cur) = current_class {
            let key = (cur.clone(), property.to_string());
            if let Some(fname) = ctx.methods.get(&key).cloned() {
                static_method = Some((fname, key));
                break;
            }
            current_class = ctx.classes.get(&cur).and_then(|c| c.extends_name.clone());
        }

        if let Some((fallback_fn, fallback_key)) = static_method {
            // Step 2: collect overriding subclasses. For each
            // subclass C transitively extending class_name, look
            // up which method C uses for `property` (walking C's
            // parent chain). If that resolves to a different
            // function than the static fallback, C needs an
            // explicit case in the dispatch table.
            let mut overrides: Vec<(u32, String)> = Vec::new();
            // Exact direct-call ABI for each override, aligned with `overrides`:
            // (has any rest-shaped param, has synthesized arguments, has USER
            // rest — the #8162 bit that sizes a two-array tail — declared
            // count).
            let mut override_meta: Vec<(bool, bool, bool, usize)> = Vec::new();
            // Class ids this module's table proves resolve `property` to the
            // static fallback: `class_name` itself, then its non-overriding
            // descendants. Only these may take the fallback body from the
            // virtual tower below. A class the table does not know (a subclass
            // declared in an importing module, or a class expression evaluated
            // elsewhere) may inherit an override from an intermediate class,
            // so its receiver resolves `property` through its prototype chain.
            let mut fallback_ids: Vec<u32> = ctx
                .class_ids
                .get(&class_name)
                .copied()
                .filter(|&id| id != 0)
                .into_iter()
                .collect();
            // Fixed order, not `HashMap` order — the virtual-override tower has
            // the same #7622 defect as the interface tower above, and for the
            // same reason: `overrides` is walked by index to emit the
            // `vdispatch.caseN` blocks, the `icmp eq i32` chain and the phi
            // incoming list, so the map's per-process order WAS the arm order.
            let mut override_roots: Vec<(&str, u32)> = ctx
                .class_hierarchy
                .descendants(&class_name, None)
                .into_iter()
                .filter_map(|name| ctx.class_ids.get(name).map(|&id| (name, id)))
                .collect();
            override_roots.sort_unstable_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(b.0)));
            for (sub_name, sub_id) in override_roots {
                // Resolve the method for sub_name by walking its
                // own parent chain (NOT class_name's chain).
                let mut cur = Some(sub_name.to_string());
                let mut sub_method: Option<(String, (String, String))> = None;
                while let Some(c) = cur {
                    let key = (c.clone(), property.to_string());
                    if let Some(fname) = ctx.methods.get(&key).cloned() {
                        sub_method = Some((fname, key));
                        break;
                    }
                    cur = ctx.classes.get(&c).and_then(|c| c.extends_name.clone());
                }
                if let Some((sub_fn, sub_key)) = sub_method {
                    if sub_fn == fallback_fn {
                        if sub_id != 0
                            && !fallback_ids.contains(&sub_id)
                            && fallback_ids.len() < MAX_SUBCLASS_DISPATCH_ARMS
                        {
                            fallback_ids.push(sub_id);
                        }
                    } else {
                        let has_rest = matches!(ctx.method_has_rest.get(&sub_key), Some(&true));
                        let has_synthetic_arguments = matches!(
                            ctx.method_has_synthetic_arguments.get(&sub_key),
                            Some(&true)
                        );
                        // `sub_key.0` is the class the walk RESOLVED to, i.e.
                        // the defining class of the body this case calls.
                        let has_user_rest = crate::codegen::arguments::method_has_user_rest(
                            ctx, &sub_key.0, property,
                        );
                        let declared_count =
                            ctx.method_param_counts.get(&sub_key).copied().unwrap_or(0);
                        overrides.push((sub_id, sub_fn));
                        override_meta.push((
                            has_rest,
                            has_synthetic_arguments,
                            has_user_rest,
                            declared_count,
                        ));
                    }
                }
            }

            // Metadata needed to state #9480's post-argument window before
            // lowering the receiver. Computing it emits no program IR, so JS
            // evaluation order remains receiver-then-arguments.
            let can_collapse_virtual = crate::codegen::full_outline_ic_enabled()
                && method_dispatch_collapse_enabled()
                && !overrides.is_empty()
                && !fallback_fn.starts_with("perry_static_")
                && overrides
                    .iter()
                    .all(|(_, f)| !f.starts_with("perry_static_"));
            let fallback_decl_count = ctx
                .method_param_counts
                .get(&fallback_key)
                .copied()
                .unwrap_or(0);
            let fallback_has_rest = matches!(ctx.method_has_rest.get(&fallback_key), Some(&true));
            let fallback_has_synthetic_arguments = matches!(
                ctx.method_has_synthetic_arguments.get(&fallback_key),
                Some(&true)
            );
            // `fallback_key.0` is the class the parent-chain walk resolved
            // `property` to: the defining class that owns #8162's user-rest
            // bit and the arguments-length-only specialization metadata.
            let fallback_has_user_rest =
                crate::codegen::arguments::method_has_user_rest(ctx, &fallback_key.0, property);
            let fallback_arguments_length_only = matches!(
                ctx.method_arguments_length_only.get(&fallback_key),
                Some(&true)
            );
            let collapses_before_bundling = (fallback_has_rest
                || override_meta.iter().any(|meta| meta.0))
                && can_collapse_virtual;
            // Even an `arguments.length`-only clone needs a real Arguments
            // bundle on a guard miss / own-override route, so keep that
            // fallback allocation in the window. The early collapse above is
            // the only path that proves no direct bundle will be emitted.
            let fallback_bundle_may_collect = fallback_has_rest || fallback_has_synthetic_arguments;
            let any_bundle_may_collect = !collapses_before_bundling
                && (fallback_bundle_may_collect
                    || override_meta.iter().any(|meta| meta.0 || meta.1));
            // Specialized direct paths can place typed-feedback/argument
            // guards between the final source argument and its consuming
            // method call. Those guards are outside #9480's two-helper audit,
            // so retain the conservative window for those uncommon paths.
            let typed_method_key = (class_name.clone(), property.to_string());
            let has_specialized_guard = ctx.typed_f64_methods.contains(&typed_method_key)
                || ctx.typed_i32_methods.contains(&typed_method_key)
                || ctx.typed_i1_methods.contains(&typed_method_key)
                || ctx.typed_string_methods.contains(&typed_method_key)
                || ctx
                    .classes
                    .get(&class_name)
                    .and_then(|class| {
                        let class = *class;
                        class
                            .methods
                            .iter()
                            .find(|method| method.name == property)
                            .and_then(|method| {
                                crate::codegen::typed_f64_receiver_method_info(
                                    class,
                                    method,
                                    ctx.classes,
                                )
                            })
                    })
                    .is_some();
            let post_args_may_collect = any_bundle_may_collect
                || has_specialized_guard
                || (can_collapse_virtual
                    && ctx.strings.call_location_for(call_byte_offset).is_some());

            // Keep the maximum declared arity only for selecting safe
            // shape-guarded/typed fast-path arms below. Direct calls no longer
            // share an ABI vector: the fallback and each virtual override are
            // adapted independently from `fallback_user_args`.
            let mut max_explicit_arity: usize = fallback_decl_count;
            let mut walk = Some(class_name.clone());
            while let Some(cur) = walk {
                let key = (cur.clone(), property.to_string());
                if let Some(&n) = ctx.method_param_counts.get(&key) {
                    if n > max_explicit_arity {
                        max_explicit_arity = n;
                    }
                    break;
                }
                walk = ctx.classes.get(&cur).and_then(|c| c.extends_name.clone());
            }
            // #7622: no `break` on the first name that carries `sub_id`. Class
            // ids are not unique over `class_ids` KEYS — a class-expression
            // self-binding alias (`var X = class _X`) and an imported class
            // registered under both its own and its local-alias name both map
            // two names to one id — so first-match-wins made this a hash-order
            // tie-break, and the loser's arity is what decides how many
            // TAG_UNDEFINED padding args EVERY emitted call in the tower
            // carries. Taking the max over all names sharing the id is both
            // order-independent and the direction this variable already means:
            // under-padding is the #235 garbage-argument bug, over-padding just
            // lets a default-param desugaring fire.
            for (sub_id, _) in &overrides {
                for (sub_name, &id) in ctx.class_ids.iter() {
                    if id != *sub_id {
                        continue;
                    }
                    if let Some(&n) = ctx
                        .method_param_counts
                        .get(&(sub_name.clone(), property.to_string()))
                    {
                        if n > max_explicit_arity {
                            max_explicit_arity = n;
                        }
                    }
                }
            }
            // Arms for the shape-guarded direct call: every class in
            // `class_name`'s subclass closure, paired with the body `property`
            // resolves to from THAT class. Includes subclasses that do NOT
            // override — a `Marker` receiver fails a `Node2D` class-id guard
            // just as hard as a `Rect` one does, and the tower it falls into
            // costs the same either way.
            //
            // The declared-class guard alone is a bet that the receiver's
            // dynamic class equals its static class. Where a base-typed
            // collection is the whole point of the hierarchy that bet loses
            // every single time, and the miss is not free: it pays a guard
            // call AND the full `js_native_call_method` tower.
            let mut subclass_arms: Vec<SubclassDispatchArm> = Vec::new();
            {
                let mut seen_ids: Vec<u32> = vec![*ctx.class_ids.get(&class_name).unwrap_or(&0)];
                let mut roots: Vec<(&str, u32)> = ctx
                    .class_hierarchy
                    .descendants(&class_name, None)
                    .into_iter()
                    .filter_map(|name| ctx.class_ids.get(name).map(|&id| (name, id)))
                    .collect();
                roots.sort_unstable_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(b.0)));
                for (sub_name, sub_id) in roots {
                    if sub_id == 0 || seen_ids.contains(&sub_id) {
                        continue;
                    }
                    let Some(keys_global) = ctx.class_keys_globals.get(sub_name).cloned() else {
                        continue;
                    };
                    // Resolve through the SUBCLASS's own chain, and remember
                    // where it landed: the rest-param shape is a property of
                    // the declaring class, and a rest-bearing target cannot be
                    // called with this site's flat, base-arity argument list.
                    let mut cur = Some(sub_name.to_string());
                    let mut resolved: Option<(String, String)> = None;
                    while let Some(c) = cur {
                        let key = (c.clone(), property.to_string());
                        if let Some(fname) = ctx.methods.get(&key).cloned() {
                            resolved = Some((c, fname));
                            break;
                        }
                        cur = ctx.classes.get(&c).and_then(|c| c.extends_name.clone());
                    }
                    let Some((decl_class, target_fn)) = resolved else {
                        continue;
                    };
                    if target_fn.starts_with("perry_static_") {
                        continue;
                    }
                    if matches!(
                        ctx.method_has_rest
                            .get(&(decl_class.clone(), property.to_string())),
                        Some(&true)
                    ) {
                        continue;
                    }
                    if ctx
                        .method_param_counts
                        .get(&(decl_class, property.to_string()))
                        .is_some_and(|&n| n > max_explicit_arity)
                    {
                        continue;
                    }
                    seen_ids.push(sub_id);
                    subclass_arms.push(SubclassDispatchArm {
                        class_id: sub_id,
                        keys_global,
                        target_fn,
                    });
                }
            }
            // A wide hierarchy would turn every callsite into a long compare
            // chain — more instruction cache than the tower call it replaces.
            // Beyond the cap the site keeps today's single-arm guard.
            if subclass_arms.len() > MAX_SUBCLASS_DISPATCH_ARMS {
                subclass_arms.clear();
            }
            // This ABI belongs to one producer-proved body. A subclass arm may
            // resolve to a different implementation, so let it take the raw
            // dynamic fallback instead of sharing the scalar-count vector.
            if fallback_arguments_length_only {
                subclass_arms.clear();
            }

            let typed_method_key = (class_name.clone(), property.to_string());
            let typed_formal_count = ctx
                .method_param_counts
                .get(&typed_method_key)
                .copied()
                .unwrap_or(max_explicit_arity);
            let typed_receiver_info = ctx.classes.get(&class_name).and_then(|class| {
                let class = *class;
                class
                    .methods
                    .iter()
                    .find(|method| method.name.as_str() == property)
                    .and_then(|method| {
                        crate::codegen::typed_f64_receiver_method_info(class, method, ctx.classes)
                    })
            });
            // Charter step 5, P4: the clone reads its receiver fields as
            // bare doubles behind the exact (class id, ShapeId) compare,
            // which pins lanes, not values. So every field it reads must
            // be an `F64` lane of the class's birth rep; an `Any` lane can
            // hold any value under the same ShapeId (#10937).
            let typed_receiver_direct_name = if typed_receiver_info.as_ref().is_some_and(|info| {
                info.fields.iter().all(|field| {
                    crate::expr::class_field_inline_guard::class_birth_slot_is_f64(
                        ctx,
                        &class_name,
                        field.index,
                    )
                })
            }) && ctx
                .methods
                .get(&typed_method_key)
                .is_some_and(|name| name == &fallback_fn)
                && args.len() == typed_formal_count
                && args.iter().all(|arg| {
                    crate::codegen::typed_arg_is_guard_candidate(
                        ctx,
                        crate::codegen::TypedParamRep::F64,
                        arg,
                    )
                }) {
                Some(crate::codegen::typed_f64_receiver_method_name(&fallback_fn))
            } else {
                None
            };
            let shape_only_guard = typed_receiver_direct_name.is_none()
                && !class_chain_has_field_named(ctx, &class_name, property);
            let mut constant_candidates = vec![(
                *ctx.class_ids.get(&class_name).unwrap_or(&0),
                fallback_fn.clone(),
            )];
            let selectable_arms = if shape_only_guard {
                subclass_arms.as_slice()
            } else {
                &[]
            };
            constant_candidates.extend(
                selectable_arms
                    .iter()
                    .map(|a| (a.class_id, a.target_fn.clone())),
            );
            // Match the dynamic consumer and the holder proof being replaced:
            // an incomplete prototype never acquired a compiled fast arm.
            let constant_cased: Vec<bool> = std::iter::once(Some(class_name.as_str()))
                .chain(selectable_arms.iter().map(|arm| {
                    ctx.class_ids
                        .iter()
                        .find(|(_, id)| **id == arm.class_id)
                        .map(|(name, _)| name.as_str())
                }))
                .map(|class| {
                    class.is_some_and(|class| {
                        !class_chain_has_field_named(ctx, class, property)
                            && crate::proto_constants::class_method_is_constant(
                                ctx, class, property,
                            )
                    })
                })
                .collect();
            // Only a family with a static holder proof enters the new
            // pre-argument selection. Native-parent and unsupported wide
            // prototypes keep their existing tower and argument lowering.
            let family_method_proof =
                crate::codegen::static_prototype::chain(ctx, &class_name, property).is_some();
            let observes_lookup =
                family_method_proof && crate::expr::method_site::args_may_observe_lookup(ctx, args);
            // An observable argument needs a selection before it runs even
            // for a single/typed arm. The later layout guard still licenses
            // that arm's representation clone independently.
            let constant_static = (shape_only_guard || observes_lookup)
                && (!selectable_arms.is_empty() || observes_lookup)
                && constant_cased.contains(&true)
                && !fallback_fn.starts_with("perry_static_")
                && !fallback_has_rest
                && !fallback_has_synthetic_arguments
                && crate::expr::method_site::method_site_enabled(ctx, property, args.len())
                && ctx.ptr_shape_receiver_fact(object).is_none();
            // A mutable method must be read before an argument can replace
            // it. The guarded direct path below tests after evaluating args;
            // use the existing split lookup/call when there is no preselected
            // constant body. This also preserves accessor/Proxy side effects.
            if family_method_proof
                && !constant_static
                && !fallback_fn.starts_with("perry_static_")
                && crate::expr::method_site::method_site_enabled(ctx, property, args.len())
                && crate::expr::method_site::args_may_observe_lookup(ctx, args)
            {
                let mut lanes =
                    crate::codegen::static_constfn::static_method_lanes(ctx, object, property);
                // Compare the actual selected function's code identity
                // and pass its real environment on this proved family path.
                if !fallback_has_rest
                    && !fallback_has_synthetic_arguments
                    && ctx.method_param_counts.contains_key(&fallback_key)
                    && fallback_fn.starts_with("perry_method_")
                    && ctx.classes.get(&fallback_key.0).is_some_and(|class| {
                        !class.is_literal_shape()
                            && class.specialized_from.is_none()
                            && class.methods.iter().any(|m| {
                                m.name == property && m.params.len() == fallback_decl_count
                            })
                    })
                {
                    let body = format!("{fallback_fn}__eclo");
                    if !lanes.iter().any(|l| l.body == body) {
                        lanes.push(crate::codegen::static_constfn::StaticMethodLane {
                            word: 0,
                            slot: 0,
                            body,
                            arity: fallback_decl_count,
                            own: false,
                        });
                    }
                }
                return crate::lower_call::lower_split_method_call(
                    ctx,
                    object,
                    property,
                    args,
                    call_byte_offset,
                    &lanes,
                    None,
                    None,
                )
                .map(Some);
            }
            let static_split =
                constant_static && crate::expr::method_site::args_may_observe_lookup(ctx, args);

            // #9417, same defect as the dynamic-dispatch site above and for the
            // same reason: the receiver is lowered first (JS evaluation order)
            // and consumed by the override probe, the guarded direct calls and
            // every arm of the class-id tower — all of it below the lowering of
            // the argument expressions, any of which can drive an evacuating
            // young-gen minor. `build_direct_method_args` additionally allocates
            // a rest array between the re-read and the call.
            //
            // One `RootedGroup` over [receiver, ...args], re-read below the
            // group. #9480 protects each operand only when a later argument or
            // an audited post-argument step can collect; `root_reload`
            // re-derives rooted values after any remaining collection point.
            // Rest/synthetic bundles, debug bookkeeping, and specialized
            // guards retain the conservative post-argument window.
            let mut roots =
                crate::rooting::open_rooted_group(1 + args.len() + usize::from(static_split));
            let recv_collects = static_split
                || post_args_may_collect
                || crate::rooting::any_operand_may_collect(ctx, args.iter());
            let recv_idx = roots.lower(ctx, object, recv_collects)?;
            let selection = if constant_static {
                let recv = roots.reread(ctx, recv_idx)?;
                let word = crate::lower_call::method_override::emit_inline_direct_method_word_probe(
                    ctx, &recv,
                );
                let key = ctx.strings.intern(property);
                let dispatch = ctx.strings.static_dispatch_global(key);
                let method_id = crate::strings::emit_static_dispatch_id(ctx.block(), &dispatch);
                let selected = select::select_with_word(
                    ctx,
                    &recv,
                    property,
                    &constant_candidates,
                    &constant_cased,
                    &word,
                );
                let lookup = if static_split {
                    let claimed = ctx.block().icmp_ne(I32, &selected, "0");
                    crate::expr::calls::emit_call_location_at(ctx, call_byte_offset);
                    let lookup = crate::expr::method_site::emit_method_site_lookup_unless(
                        ctx, property, &recv, &method_id, &claimed,
                    );
                    let value = lookup.value.clone();
                    let value_root = roots.adopt_emitted(
                        ctx,
                        crate::rooting::Repr::Boxed,
                        &value,
                        crate::rooting::any_operand_may_collect(ctx, args.iter()),
                    );
                    Some((lookup, value_root))
                } else {
                    None
                };
                Some((selected, word, lookup))
            } else {
                None
            };
            let mut arg_idxs: Vec<usize> = Vec::with_capacity(args.len());
            for (index, a) in args.iter().enumerate() {
                let collects = post_args_may_collect
                    || crate::rooting::any_operand_may_collect(ctx, args[index + 1..].iter());
                arg_idxs.push(roots.lower(ctx, a, collects)?);
            }
            let recv_box = roots.reread(ctx, recv_idx)?;
            let mut fallback_user_args: Vec<String> = Vec::with_capacity(args.len());
            for &idx in &arg_idxs {
                fallback_user_args.push(roots.reread(ctx, idx)?);
            }
            // Collapse a rest-bearing virtual dispatch HERE, before the rest
            // arrays are materialized below — the method site takes the raw
            // `fallback_user_args` (its miss does the rest-bundling), so the
            // bundle would be dead. (The non-rest collapse happens at the vdispatch
            // site below, where there is no array to skip.)
            if (fallback_has_rest || override_meta.iter().any(|meta| meta.0))
                && can_collapse_virtual
            {
                let collapsed = crate::lower_call::console_promise::emit_native_method_str_dispatch(
                    ctx,
                    property,
                    call_byte_offset,
                    &recv_box,
                    &fallback_user_args,
                );
                roots.release(ctx);
                return Ok(Some(collapsed));
            }
            let undefined_lit = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
            let direct_args = build_direct_method_args(
                ctx,
                &recv_box,
                &fallback_user_args,
                fallback_has_rest,
                fallback_has_synthetic_arguments,
                fallback_has_user_rest,
                fallback_arguments_length_only,
                fallback_decl_count,
                &undefined_lit,
            );
            let arg_slices: Vec<(crate::types::LlvmType, &str)> =
                direct_args.iter().map(|s| (DOUBLE, s.as_str())).collect();
            let arguments_length_direct_fn = fallback_arguments_length_only
                .then(|| crate::codegen::arguments::arguments_length_method_name(&fallback_fn));

            // Object model step 3 (§3.6): the receiver's family set is proven
            // to be exactly `{class_name}` and the method a prototype
            // constant, so the call binds the body with no load for dispatch
            // and no guard. Both the public method and its arguments-length
            // clone accept any receiver layout; select the matching ABI.
            // A `Ptr<Shape>` receiver keeps its own route,
            // which also proves the layout and calls the layout-specialised
            // body.
            // With a dynamic write or relink in the program the binding needs
            // the shadow-bit test; the guarded direct call below makes it
            // (exact receiver word compare + `holder_constant`).
            if ctx.exact_receiver_class(object).as_deref() == Some(class_name.as_str())
                && !crate::proto_constants::shadow_tests()
                && ctx.ptr_shape_receiver_fact(object).is_none()
                && !fallback_fn.starts_with("perry_static_")
                && !class_chain_has_field_named(ctx, &class_name, property)
                && crate::proto_constants::class_method_is_constant(ctx, &class_name, property)
            {
                if crate::codegen::helpers::check_contracts() {
                    crate::proto_constants::emit_exact_family_contract(
                        ctx,
                        &recv_box,
                        &class_name,
                        property,
                        &fallback_fn,
                    );
                }
                let target = arguments_length_direct_fn
                    .as_deref()
                    .unwrap_or(&fallback_fn);
                let value = ctx.block().call(DOUBLE, target, &arg_slices);
                roots.release(ctx);
                return Ok(Some(value));
            }

            // A synthesized `arguments` slot is rest-shaped in the HIR, but
            // it does not prevent a same-class guarded direct call.  The
            // concrete ABI vector above already contains the correctly marked
            // argument bundle, while the guard's miss path deliberately takes
            // the original flat user arguments through
            // `js_native_call_method_by_id` so an own override receives normal
            // JavaScript call arguments.  Keep genuine user-rest methods and
            // all mixed user-rest + `arguments` methods on the established
            // dynamic path; their direct-call specialization remains a
            // separate ABI problem.
            let synth_arguments_only = fallback_has_synthetic_arguments && !fallback_has_user_rest;
            if !fallback_has_rest || synth_arguments_only {
                let typed_direct_name = if ctx.typed_f64_methods.contains(&typed_method_key)
                    && ctx
                        .methods
                        .get(&typed_method_key)
                        .is_some_and(|name| name == &fallback_fn)
                    && ctx
                        .typed_i1_method_param_reps
                        .get(&typed_method_key)
                        .is_some_and(|reps| {
                            crate::codegen::typed_param_reps_match_args(ctx, reps, args)
                        }) {
                    Some(crate::codegen::typed_f64_method_name(&fallback_fn))
                } else {
                    None
                };
                let typed_i32_direct_name = if ctx.typed_i32_methods.contains(&typed_method_key)
                    && ctx
                        .methods
                        .get(&typed_method_key)
                        .is_some_and(|name| name == &fallback_fn)
                    && ctx
                        .typed_i1_method_param_reps
                        .get(&typed_method_key)
                        .is_some_and(|reps| {
                            crate::codegen::typed_param_reps_match_args(ctx, reps, args)
                        }) {
                    Some(crate::codegen::typed_i32_method_name(&fallback_fn))
                } else {
                    None
                };
                let typed_i1_direct_name = if ctx.typed_i1_methods.contains(&typed_method_key)
                    && ctx
                        .methods
                        .get(&typed_method_key)
                        .is_some_and(|name| name == &fallback_fn)
                    && ctx
                        .typed_i1_method_param_reps
                        .get(&typed_method_key)
                        .is_some_and(|reps| {
                            crate::codegen::typed_param_reps_match_args(ctx, reps, args)
                        }) {
                    Some(crate::codegen::typed_i1_method_name(&fallback_fn))
                } else {
                    None
                };
                let typed_string_direct_name = if ctx
                    .typed_string_methods
                    .contains(&typed_method_key)
                    && ctx
                        .methods
                        .get(&typed_method_key)
                        .is_some_and(|name| name == &fallback_fn)
                    && ctx
                        .typed_i1_method_param_reps
                        .get(&typed_method_key)
                        .is_some_and(|reps| {
                            crate::codegen::typed_param_reps_match_args(ctx, reps, args)
                        }) {
                    Some(crate::codegen::typed_string_method_name(&fallback_fn))
                } else {
                    None
                };
                let typed_direct = typed_direct_name.as_ref().and_then(|name| {
                    ctx.typed_i1_method_param_reps
                        .get(&typed_method_key)
                        .cloned()
                        .map(|reps| (name.as_str(), reps))
                });
                let nonnegative_index_direct_name = ctx
                    .nonnegative_index_methods
                    .get(&typed_method_key)
                    .and_then(|params| {
                        let method = ctx
                            .classes
                            .get(&class_name)?
                            .methods
                            .iter()
                            .find(|method| method.name.as_str() == property)?;
                        if args.len() != method.params.len()
                            || !ctx
                                .methods
                                .get(&typed_method_key)
                                .is_some_and(|name| name == &fallback_fn)
                        {
                            return None;
                        }
                        let all_proven = params.iter().all(|id| {
                            method
                                .params
                                .iter()
                                .position(|param| param.id == *id)
                                .and_then(|position| args.get(position))
                                .is_some_and(|arg| {
                                    crate::expr::numeric_index_has_integer_array_index_proof(
                                        ctx, arg,
                                    )
                                })
                        });
                        all_proven.then(|| {
                            crate::codegen::nonnegative_index_method_name(&fallback_fn, params)
                        })
                    });
                // A versioned loop revalidated this exact `this.method` target
                // and every array argument at the current iteration entry.
                // Route directly to the private handle-ABI clone: unlike the
                // ordinary `$idx_u31` body it contains no array fallback edge.
                // The structural matcher admits no user code between that
                // revalidation and these checked-reader calls.
                let versioned_fact = ctx.versioned_indexed_loop_facts.last().cloned();
                if matches!(object, Expr::This)
                    && nonnegative_index_direct_name.is_some()
                    && versioned_fact.as_ref().is_some_and(|fact| {
                        fact.method.class_name == class_name && fact.method.method_name == property
                    })
                {
                    let fact = versioned_fact.expect("checked above");
                    let params = ctx
                        .nonnegative_index_methods
                        .get(&typed_method_key)
                        .expect("versioned method remains indexed");
                    let method = ctx
                        .classes
                        .get(&class_name)
                        .expect("versioned method class remains registered")
                        .methods
                        .iter()
                        .find(|method| method.name.as_str() == property)
                        .expect("versioned method remains registered");
                    let array_params =
                        crate::codegen::nonnegative_index_fast_array_params(method, params);
                    let mut handle_storage = Vec::with_capacity(array_params.len());
                    for array_param in array_params {
                        let position = method
                            .params
                            .iter()
                            .position(|param| param.id == array_param)
                            .expect("versioned array parameter remains registered");
                        let Some(Expr::LocalGet(local_id)) = args.get(position) else {
                            handle_storage.clear();
                            break;
                        };
                        let Some(handle) = fact.live_array_handles.get(local_id) else {
                            handle_storage.clear();
                            break;
                        };
                        handle_storage.push(handle.clone());
                    }
                    if !handle_storage.is_empty() {
                        let mut fast_args = arg_slices.clone();
                        fast_args
                            .extend(handle_storage.iter().map(|handle| (I64, handle.as_str())));
                        let target = crate::codegen::nonnegative_index_fast_array_method_name(
                            &fallback_fn,
                            params,
                        );
                        let value = if matches!(
                            fact.guard_mode,
                            crate::expr::VersionedIndexedGuardMode::CallbackDeopt { .. }
                        ) {
                            // Admission proved every private handle argument
                            // is a live in-bounds array. The checked-reader
                            // clone's only collecting arms are therefore
                            // unreachable in this loop version; a callback
                            // cold arm side-exits before the next use.
                            ctx.block().call_gc_leaf(DOUBLE, &target, &fast_args)
                        } else {
                            ctx.block().call(DOUBLE, &target, &fast_args)
                        };
                        roots.release(ctx);
                        return Ok(Some(value));
                    }
                }
                let typed_receiver_direct = match (
                    typed_receiver_direct_name.as_ref(),
                    typed_receiver_info.as_ref(),
                ) {
                    (Some(name), Some(info)) => Some((name.as_str(), typed_formal_count, info)),
                    _ => None,
                };
                let typed_i32_direct = typed_i32_direct_name.as_ref().and_then(|name| {
                    ctx.typed_i1_method_param_reps
                        .get(&typed_method_key)
                        .cloned()
                        .map(|reps| (name.as_str(), reps))
                });
                let typed_i1_direct = typed_i1_direct_name.as_ref().and_then(|name| {
                    ctx.typed_i1_method_param_reps
                        .get(&typed_method_key)
                        .cloned()
                        .map(|reps| (name.as_str(), reps))
                });
                let typed_string_direct = typed_string_direct_name.as_ref().and_then(|name| {
                    ctx.typed_i1_method_param_reps
                        .get(&typed_method_key)
                        .cloned()
                        .map(|reps| (name.as_str(), reps))
                });
                // Representation-selection Phase 3b: shape-proven Ptr<Shape>
                // receiver (collectors/ptr_shape.rs) — direct call with NO
                // shape guard and NO own-override probe. Provenance proves
                // the receiver's dynamic class is exactly `class_name`
                // (subclass overrides cannot apply), the eligibility walk
                // vetted every method called on the local (chain-resolvable,
                // `this`-flow safe), no own-property write can shadow the
                // method (non-declared-field writes disqualify), and
                // `prototype_is_stable` held for the chain.
                // Phase 5a additionally admits `this` inside a `$pshape`
                // clone, so a proven receiver's `this.other()` chain stays
                // guard-free instead of falling back to the dispatch tower.
                let ptr_shape_receiver = ctx
                    .ptr_shape_receiver_fact(object)
                    .map(|fact| fact.class_name == class_name)
                    .unwrap_or(false);
                if ptr_shape_receiver && !fallback_fn.starts_with("perry_static_") {
                    ctx.note_ptr_shape_consumed(object, "ptr_shape_method");
                    if let Some(target) = arguments_length_direct_fn.as_deref() {
                        let value = ctx.block().call(DOUBLE, target, &arg_slices);
                        roots.release(ctx);
                        return Ok(Some(value));
                    }
                    // Representation-selection Phase 5a: the proven-`this`
                    // clone, when one was emitted. Hoisted above the
                    // typed-receiver branch because BOTH exits of this block
                    // are guard-free under the same Phase 3b proof — the
                    // typed-receiver arm's own generic fallback used to call
                    // the guard-ridden public body while the plain exit 25
                    // lines below routed to the clone, so a proven receiver
                    // whose ARGUMENTS happened to be non-plain-double silently
                    // lost the receiver proof too.
                    //
                    // A `pshape_methods` hit additionally proves `class_name`
                    // DECLARES `property` — so the clone's `this` is exactly
                    // the class it was compiled for and cannot be a subclass
                    // instance.
                    let pshape_target = ctx
                        .pshape_methods
                        .contains_key(&(class_name.clone(), property.to_string()))
                        .then(|| crate::collectors::pshape_method_name(&fallback_fn));
                    // #8607: containment is stronger than the exact-shape
                    // proof used by the other `$pshape` routes. Only here can
                    // a method safely keep array-valued receiver fields in
                    // locals across calls: no alias exists that could replace
                    // a slot while the method runs. Emission and routing use
                    // the same structural eligibility predicate.
                    let ptr_array_cache_target = pshape_target.as_ref().and_then(|_| {
                        let class = ctx.classes.get(&class_name)?;
                        let method = class.methods.iter().find(|m| m.name == property)?;
                        (!crate::collectors::ptr_array_cache_fields(class, method).is_empty())
                            .then(|| crate::collectors::ptr_array_cache_method_name(&fallback_fn))
                    });
                    let pshape_index_target =
                        nonnegative_index_direct_name.as_ref().and_then(|_| {
                            let pshape = pshape_target.as_ref()?;
                            let params = ctx.nonnegative_index_methods.get(&typed_method_key)?;
                            Some(crate::codegen::nonnegative_index_method_name(
                                pshape, params,
                            ))
                        });
                    let generic_target = pshape_index_target
                        .as_deref()
                        .or(nonnegative_index_direct_name.as_deref())
                        .or(ptr_array_cache_target.as_deref())
                        .or(pshape_target.as_deref())
                        .unwrap_or(fallback_fn.as_str());
                    // #8774: containment proves the receiver here, while the
                    // dedicated argument guards prove every selected callee
                    // parameter.  A miss calls the same receiver-safe generic
                    // target this block used before argument specialization.
                    if let Some(argument_specialized) = emit_pshape_argument_dispatch(
                        ctx,
                        &class_name,
                        property,
                        &fallback_fn,
                        generic_target,
                        &arg_slices,
                        args,
                    ) {
                        super::super::method_override::publish_constructive_method_truthy(
                            ctx,
                            &fallback_fn,
                            &argument_specialized,
                        );
                        roots.release(ctx);
                        return Ok(Some(argument_specialized));
                    }
                    // Prefer the typed-receiver clone (bare gep+load field
                    // access inside the body) when one exists: the receiver
                    // is proven, so only the ARGUMENT value classes need
                    // vetting — an inline plain-finite check per argument (no
                    // calls, no header loads). Any non-plain-double argument
                    // (NaN-box tag, INT32-boxed, NaN/Inf) falls to the
                    // generic direct call — still guard-free.
                    if let Some((typed_fn, typed_formal_count, _info)) = typed_receiver_direct {
                        let arg_values: Vec<String> = arg_slices
                            .iter()
                            .skip(1)
                            .take(typed_formal_count)
                            .map(|(_, v)| (*v).to_string())
                            .collect();
                        let obj_handle = {
                            let blk = ctx.block();
                            let obj_bits = blk.bitcast_double_to_i64(&recv_box);
                            blk.and(I64, &obj_bits, crate::nanbox::POINTER_MASK_I64)
                        };
                        let mut all_plain: Option<String> = None;
                        for v in &arg_values {
                            let blk = ctx.block();
                            let bits = blk.bitcast_double_to_i64(v);
                            let ok = crate::expr::class_field_inline_guard::
                                emit_plain_finite_number_check(blk, &bits);
                            all_plain = Some(match all_plain {
                                Some(prev) => ctx.block().and(crate::types::I1, &prev, &ok),
                                None => ok,
                            });
                        }
                        let shape_idx = ctx.new_block("ptr_shape_method.shape");
                        let typed_idx = ctx.new_block("ptr_shape_method.typed");
                        let generic_idx = ctx.new_block("ptr_shape_method.generic");
                        let merge_idx = ctx.new_block("ptr_shape_method.merge");
                        let shape_label = ctx.block_label(shape_idx);
                        let typed_label = ctx.block_label(typed_idx);
                        let generic_label = ctx.block_label(generic_idx);
                        let merge_label = ctx.block_label(merge_idx);
                        match all_plain {
                            Some(cond) => ctx.block().cond_br(&cond, &shape_label, &generic_label),
                            None => ctx.block().br(&shape_label),
                        }
                        // Provenance proves the receiver's CLASS, not its
                        // field representations: an alias the local escaped
                        // to may store a non-Number into a field, which moves
                        // the object off its birth shape (`field_rep_store`).
                        // The clone reads its fields as raw doubles, so it
                        // runs only while the (class id, ShapeId) pair is the
                        // class's own — the shape pins the `F64` lanes.
                        ctx.current_block = shape_idx;
                        let class_id = ctx.class_ids.get(&class_name).copied();
                        let keys_global = ctx.class_keys_globals.get(&class_name).cloned();
                        match (class_id, keys_global) {
                            (Some(class_id), Some(keys_global)) => {
                                let obj_bits = ctx.block().bitcast_double_to_i64(&recv_box);
                                crate::expr::class_field_inline_guard::emit_class_field_read_precheck(
                                    ctx,
                                    &obj_bits,
                                    &class_id.to_string(),
                                    true,
                                    &typed_label,
                                    &[],
                                    &keys_global,
                                );
                                ctx.block().br(&generic_label);
                            }
                            _ => ctx.block().br(&generic_label),
                        }
                        ctx.current_block = typed_idx;
                        let mut typed_args: Vec<(crate::types::LlvmType, &str)> =
                            vec![(I64, obj_handle.as_str())];
                        for v in &arg_values {
                            typed_args.push((DOUBLE, v.as_str()));
                        }
                        let v_typed = ctx.block().call(DOUBLE, typed_fn, &typed_args);
                        let typed_end = ctx.block().label.clone();
                        ctx.block().br(&merge_label);
                        ctx.current_block = generic_idx;
                        let v_generic = ctx.block().call(DOUBLE, generic_target, &arg_slices);
                        let generic_end = ctx.block().label.clone();
                        ctx.block().br(&merge_label);
                        ctx.current_block = merge_idx;
                        let merged = ctx.block().phi(
                            DOUBLE,
                            &[
                                (v_typed.as_str(), &typed_end),
                                (v_generic.as_str(), &generic_end),
                            ],
                        );
                        super::super::method_override::publish_constructive_method_truthy(
                            ctx,
                            &fallback_fn,
                            &merged,
                        );
                        roots.release(ctx);
                        return Ok(Some(merged));
                    }
                    // Representation-selection Phase 5a: route to the
                    // proven-`this` clone when one exists (`generic_target`,
                    // resolved at the top of this block). The receiver is
                    // already proven here — no guard is emitted on this path at
                    // all. Same ABI, so the call is unchanged apart from the
                    // callee name.
                    let direct = ctx.block().call(DOUBLE, generic_target, &arg_slices);
                    super::super::method_override::publish_constructive_method_truthy(
                        ctx,
                        &fallback_fn,
                        &direct,
                    );
                    roots.release(ctx);
                    return Ok(Some(direct));
                }
                let selected_value = selection
                    .as_ref()
                    .and_then(|(_, _, lookup)| lookup.as_ref())
                    .map(|(_, root)| roots.reread_emitted(ctx, *root));
                let certificate = selection.as_ref().map(|(selected, word, lookup)| {
                    crate::lower_call::method_override::ConstantMethodSelection {
                        selected,
                        receiver_word: (!static_split).then_some(word.as_str()),
                        lookup: lookup
                            .as_ref()
                            .zip(selected_value.as_ref())
                            .map(|((lookup, _), value)| (lookup, value.as_str())),
                        call_byte_offset,
                    }
                });
                if let Some(guarded) = emit_guarded_direct_method_call(
                    ctx,
                    &recv_box,
                    &class_name,
                    property,
                    &fallback_fn,
                    arguments_length_direct_fn.as_deref(),
                    &arg_slices,
                    args,
                    &fallback_user_args,
                    if fallback_arguments_length_only {
                        None
                    } else {
                        nonnegative_index_direct_name.as_deref()
                    },
                    if fallback_arguments_length_only {
                        None
                    } else {
                        typed_direct
                    },
                    if fallback_arguments_length_only {
                        None
                    } else {
                        typed_receiver_direct
                    },
                    if fallback_arguments_length_only {
                        None
                    } else {
                        typed_i32_direct
                    },
                    if fallback_arguments_length_only {
                        None
                    } else {
                        typed_i1_direct
                    },
                    if fallback_arguments_length_only {
                        None
                    } else {
                        typed_string_direct
                    },
                    shape_only_guard,
                    &subclass_arms,
                    certificate.as_ref(),
                ) {
                    roots.release(ctx);
                    return Ok(Some(guarded));
                }
            }

            // No direct/proven route consumed the scalar ABI. Materialize the
            // marked public Arguments bundle only for the established
            // own-override/static paths below.
            drop(arg_slices);
            let fallback_args = if fallback_arguments_length_only {
                build_direct_method_args(
                    ctx,
                    &recv_box,
                    &fallback_user_args,
                    fallback_has_rest,
                    fallback_has_synthetic_arguments,
                    fallback_has_user_rest,
                    false,
                    fallback_decl_count,
                    &undefined_lit,
                )
            } else {
                direct_args
            };
            let arg_slices: Vec<(crate::types::LlvmType, &str)> =
                fallback_args.iter().map(|s| (DOUBLE, s.as_str())).collect();

            // With no override in this module's class table the tower below
            // has no case arm; it still tests the receiver's class id, because
            // a class the table does not know (declared in an importing module,
            // or evaluated from a class expression elsewhere) may override
            // `property` itself.
            // #5391 path 4 (virtual tower): the oversized-module outline emits
            // the universal method site instead of the per-overriding-subclass
            // class-id switch below. The site's memo answers each receiver
            // shape with its class prototype's slot (an override is just
            // another shape's entry), and its miss is the by-name dispatch.
            // The rest-bearing case already collapsed above (before the rest
            // bundling); this handles the non-rest case.
            if can_collapse_virtual {
                let collapsed = crate::lower_call::console_promise::emit_native_method_str_dispatch(
                    ctx,
                    property,
                    call_byte_offset,
                    &recv_box,
                    &fallback_user_args,
                );
                roots.release(ctx);
                return Ok(Some(collapsed));
            }

            // Step 4: virtual dispatch via class_id switch.
            // Read class_id from the object header, then branch
            // to the right concrete method block.
            // Pre-create blocks: one per override + default + merge.
            let mut case_idxs: Vec<usize> = Vec::with_capacity(overrides.len());
            for (i, _) in overrides.iter().enumerate() {
                case_idxs.push(ctx.new_block(&format!("vdispatch.case{}", i)));
            }
            let default_idx = ctx.new_block("vdispatch.default");
            let merge_idx = ctx.new_block("vdispatch.merge");

            // POINTER_TAG (0x7FFD) guard: a receiver statically typed as a class
            // can still hold a primitive at runtime (TS type-vs-value drift). For
            // a non-pointer receiver, masking + `js_object_get_class_id` derefs
            // garbage and crashes (EXC_BAD_ACCESS). Route non-pointers to the
            // static fallback (`vdispatch.default`). Same idiom as
            // `expr/class_field_inline_guard.rs` (lshr 48; icmp eq 0x7FFD).
            let tower_idx = ctx.new_block("vdispatch.tower");
            let tower_label = ctx.block_label(tower_idx);
            let default_label_guard = ctx.block_label(default_idx);
            let merge_label = ctx.block_label(merge_idx);
            let mut phi_inputs: Vec<(String, String)> = Vec::new();
            // Issue #620: an own-property method (`this.method = X`, or
            // `class X { method = fn; }`) wins over every prototype body, so
            // it is probed first. Hono's SmartRouter rebinds `this.match` on
            // the first call. The override is a dynamic value (arrow / bound /
            // native method) and receives the FLAT user arguments
            // (`fallback_user_args`), not the ABI-adapted bundle, so a native
            // override such as `super.emit(event, ...args)` forwarding to
            // EventEmitter sees ordinary call arguments.
            {
                let (dir, key_bits, hash) = crate::expr::method_site::emit_site_key(ctx, property);
                let own_method = ctx.block().call(
                    DOUBLE,
                    "perry_probe_own_method",
                    &[
                        (crate::types::PTR, &dir),
                        (DOUBLE, &recv_box),
                        (I64, &key_bits),
                        (I64, &hash),
                    ],
                );
                let own_bits = ctx.block().bitcast_double_to_i64(&own_method);
                let undef_bits = format!("{}", crate::nanbox::TAG_UNDEFINED as i64);
                let is_undef = ctx.block().icmp_eq(I64, &own_bits, &undef_bits);
                let own_idx = ctx.new_block("vdispatch.own");
                let class_idx = ctx.new_block("vdispatch.class");
                let own_label = ctx.block_label(own_idx);
                let class_label = ctx.block_label(class_idx);
                ctx.block().cond_br(&is_undef, &class_label, &own_label);
                ctx.current_block = own_idx;
                let v_own = crate::lower_call::method_override::emit_own_override_value_call(
                    ctx,
                    &own_method,
                    &recv_box,
                    &fallback_user_args,
                );
                let own_end = ctx.block().label.clone();
                if !ctx.block().is_terminated() {
                    ctx.block().br(&merge_label);
                }
                phi_inputs.push((v_own, own_end));
                ctx.current_block = class_idx;
            }
            {
                let blk = ctx.block();
                let recv_bits = blk.bitcast_double_to_i64(&recv_box);
                let recv_tag = blk.lshr(I64, &recv_bits, "48");
                let is_ptr = blk.icmp_eq(I64, &recv_tag, "32765"); // 0x7FFD POINTER_TAG
                blk.cond_br(&is_ptr, &tower_label, &default_label_guard);
            }

            ctx.current_block = tower_idx;
            let blk = ctx.block();
            let recv_handle = unbox_to_i64(blk, &recv_box);
            let cid = blk.call(I32, "js_object_get_class_id", &[(I64, &recv_handle)]);

            // Default → fallback. We use a tower of icmp+br rather
            // than the LLVM `switch` instruction (which the IR
            // builder doesn't expose generically) — same shape,
            // slightly more verbose.
            // After the override arms: the ids proven to resolve to the
            // fallback take it; every other class id resolves `property`
            // through the receiver's own prototype chain (the universal
            // method site, whose per-site memo answers a repeated shape).
            let known_idx = ctx.new_block("vdispatch.known");
            let known_label = ctx.block_label(known_idx);
            let mut current_label = ctx.block().label.clone();
            for (i, (case_cid, _)) in overrides.iter().enumerate() {
                let next_label = if i + 1 < overrides.len() {
                    // We'll start the next test in this same block
                    // — actually use a fresh block for the test.
                    format!("vdispatch.test{}", i + 1)
                } else {
                    known_label.clone()
                };
                let case_label = ctx.block_label(case_idxs[i]);
                // Make sure ctx.current_block points at the
                // current test block.
                let _ = current_label;
                let cmp = ctx.block().icmp_eq(I32, &cid, &case_cid.to_string());
                if i + 1 < overrides.len() {
                    // Create the next test block as a fresh block
                    // and branch into it on the false arm.
                    let next_idx = ctx.new_block(&format!("vdispatch.test{}", i + 1));
                    let next_lbl = ctx.block_label(next_idx);
                    ctx.block().cond_br(&cmp, &case_label, &next_lbl);
                    ctx.current_block = next_idx;
                    current_label = next_lbl;
                } else {
                    ctx.block().cond_br(&cmp, &case_label, &next_label);
                }
            }

            if overrides.is_empty() {
                ctx.block().br(&known_label);
            }
            let generic_idx = ctx.new_block("vdispatch.generic");
            let generic_label = ctx.block_label(generic_idx);
            ctx.current_block = known_idx;
            for (i, known_cid) in fallback_ids.iter().enumerate() {
                let miss_label = if i + 1 < fallback_ids.len() {
                    let next_idx = ctx.new_block(&format!("vdispatch.known{}", i + 1));
                    Some((next_idx, ctx.block_label(next_idx)))
                } else {
                    None
                };
                let cmp = ctx.block().icmp_eq(I32, &cid, &known_cid.to_string());
                let miss = miss_label
                    .as_ref()
                    .map_or(generic_label.as_str(), |(_, label)| label.as_str());
                ctx.block().cond_br(&cmp, &default_label_guard, miss);
                if let Some((next_idx, _)) = miss_label {
                    ctx.current_block = next_idx;
                }
            }
            if fallback_ids.is_empty() {
                ctx.block().br(&generic_label);
            }

            // Each case block: adapt the raw user arguments to THIS override's
            // declared ABI, call it, and branch to merge. In particular, a
            // synthesized `arguments` array belongs only to implementations
            // that declare that hidden slot; it cannot be inherited from the
            // fallback's signature or shared with a sibling override.
            for (
                ((_, fname), &(has_rest, has_synthetic_arguments, has_user_rest, declared_count)),
                &case_idx,
            ) in overrides
                .iter()
                .zip(override_meta.iter())
                .zip(case_idxs.iter())
            {
                ctx.current_block = case_idx;
                let case_args = build_direct_method_args(
                    ctx,
                    &recv_box,
                    &fallback_user_args,
                    has_rest,
                    has_synthetic_arguments,
                    has_user_rest,
                    false,
                    declared_count,
                    &undefined_lit,
                );
                let case_arg_slices: Vec<(crate::types::LlvmType, &str)> = case_args
                    .iter()
                    .map(|argument| (DOUBLE, argument.as_str()))
                    .collect();
                let v = ctx.block().call(DOUBLE, fname, &case_arg_slices);
                let after_label = ctx.block().label.clone();
                if !ctx.block().is_terminated() {
                    ctx.block().br(&merge_label);
                }
                phi_inputs.push((v, after_label));
            }

            // Default block: call the static fallback.
            ctx.current_block = default_idx;
            let v_def = ctx.block().call(DOUBLE, &fallback_fn, &arg_slices);
            let def_label = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&merge_label);
            }
            phi_inputs.push((v_def, def_label));

            ctx.current_block = generic_idx;
            let v_generic = crate::lower_call::console_promise::emit_native_method_str_dispatch(
                ctx,
                property,
                call_byte_offset,
                &recv_box,
                &fallback_user_args,
            );
            let generic_end = ctx.block().label.clone();
            if !ctx.block().is_terminated() {
                ctx.block().br(&merge_label);
            }
            phi_inputs.push((v_generic, generic_end));

            // Merge: phi over all incoming case results.
            ctx.current_block = merge_idx;
            let phi_args: Vec<(&str, &str)> = phi_inputs
                .iter()
                .map(|(v, l)| (v.as_str(), l.as_str()))
                .collect();
            let v_tower = ctx.block().phi(DOUBLE, &phi_args);
            // The release post-dominates every case block and the default arm,
            // which is why the group is `open_rooted_group`: releasing inside an
            // arm would cut slots a sibling arm still reads.
            roots.release(ctx);
            return Ok(Some(v_tower));
        }
    }
    Ok(None)
}
