//! Issue #636: namespace member call —
//! `Call { callee: PropertyGet { ExternFuncRef(ns), method }, args }`
//! where `ns ∈ namespace_imports`.
//!
//! # Layer 1 migration (#7615)
//!
//! Every rooting decision this file makes goes through `crate::rooting`. The
//! three `lower_exprs_rooted` + `temp_root_release` pairs in the `timers` arms
//! became [`with_operands_rooted`], which owns the release on every path out.
//!
//! **The audit that earned the ledger line matters more than the translation.**
//! Only the three `timers` arms were rooted at all; every other arm in this file
//! lowered its operands into bare SSA registers and then held them across more
//! user code. Five distinct windows, all of them the shapes
//! `docs/src/internals/gc-rooting-invariant.md` names:
//!
//!  1. **`fs/promises` `writeFile` / `appendFile` / `rmdir`** — operand-to-operand.
//!     `path` is lowered, then `content` and `options` lower arbitrary user
//!     code, then `path` is read by the consuming call.
//!  2. **Both V8-bridge arms** — `for a in args { lowered.push(lower_expr(a)?) }`
//!     with no protection at all. This is #7240's shape verbatim, in a loop that
//!     post-dates the fix.
//!  3. **The var-shaped namespace export** (`imported_vars`) — the worst of the
//!     five. The closure is fetched from its zero-arg getter FIRST (spec order:
//!     the callee reference is evaluated before the arguments), sits in a bare
//!     register across every argument's lowering, and is only then `unbox_to_i64`'d
//!     into a RAW heap address and dispatched. It is #7280 taxonomy (a) and (c) at
//!     once: `root_reload` could not have repaired it even if it had been reached,
//!     because the pointer is derived below the window from a register captured
//!     above it.
//!  4. **The `has_rest` direct-call arm** — the #7154 accumulator shape, verbatim:
//!     `current` is a raw `*mut ArrayHeader` threaded through a push loop while
//!     the NEXT argument's expression is lowered, holding the only reference to
//!     everything pushed so far. `super::lower_rest_call_args_rooted` was written
//!     for exactly this and this path never adopted it.
//!  5. **The non-rest direct-call arm** — the plain unprotected argument loop.
//!
//! Windows are counted in **emissions**, not source lines. The `clear*` arm and
//! the one-argument `setImmediate` arm lower a single operand and consume it in
//! the very next emission, so they have no window and are deliberately left on
//! bare `lower_expr`; routing them through the API would emit nothing anyway
//! (`any_may_trigger_gc` over an empty tail is `false`) and would only suggest a
//! protection that is not there to give.
//!
//! One boundary is stated rather than hidden: the `has_rest` arm delegates to
//! `super::lower_rest_call_args_rooted`, which still names the raw API because
//! `crate::rooting` cannot yet express the variadic shape — the rest array's
//! per-element pushes each allocate, so every OTHER operand must be re-read
//! between them, and `with_operands_rooted` has exactly one re-read point. That
//! is a gap in the API, filed with the slice, not a decision this file makes.
//! Delegating to an audited helper is the same posture as calling `lower_expr`.

use anyhow::{bail, Result};
use perry_hir::Expr;

use super::extern_timers::fill_arg_buffer;
use crate::expr::{lower_expr, nanbox_pointer_inline, unbox_to_i64, FnCtx};
use crate::nanbox::double_literal;
use crate::rooting::with_operands_rooted;
use crate::types::{DOUBLE, I32, I64, PTR};

/// The NaN-boxed `undefined` literal — this file's most repeated expression.
fn undefined_lit() -> String {
    double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED))
}

pub fn try_lower_namespace_member_call(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    args: &[Expr],
) -> Result<Option<String>> {
    // Issue #636: namespace member call —
    // `Call { callee: PropertyGet { ExternFuncRef(ns), method }, args }`
    // where `ns ∈ namespace_imports`. Pre-fix this fell through to the
    // generic method-dispatch path which lower_expr'd the namespace as
    // its TAG_TRUE/stub-object value and then did `js_native_call_method`
    // with `method` against a non-callable receiver — TypeError or
    // silent 0 return.
    //
    // Resolution: route to the source's exported `method`. If `method`
    // is a var (let/const-bound closure — the canonical
    // `export const make = (s) => ...` shape), fetch the closure value
    // via the zero-arg getter `perry_fn_<src>__<method>()` and invoke
    // through `js_closure_callN`. If it's a function declaration
    // (`export function make(s)`), call the symbol directly with rest
    // bundling — same as the existing FuncRef path.
    let Expr::PropertyGet {
        object, property, ..
    } = callee
    else {
        return Ok(None);
    };
    let Expr::ExternFuncRef { name: ns_name, .. } = object.as_ref() else {
        return Ok(None);
    };
    if !ctx.namespace_imports.contains(ns_name) {
        return Ok(None);
    }
    if ctx
        .namespace_node_submodules
        .get(ns_name)
        .is_some_and(|submod| submod == "timers")
    {
        match property.as_str() {
            // #7210: the `timers` namespace forms carry the same two unrooted
            // windows as the global `setTimeout`/`setInterval`/`setImmediate`
            // lowerings in `extern_func.rs` (see the comment there), plus one
            // of their own: `cb_handle` is `unbox_to_i64`'d — a RAW heap
            // address, not even NaN-boxed — before the trailing arguments are
            // lowered. Lower the whole list through `lower_exprs_rooted` and
            // unbox below it, so the handle is derived from a post-collection
            // value.
            "setTimeout" if !args.is_empty() => {
                let arg_refs: Vec<&Expr> = args.iter().collect();
                let boxed = with_operands_rooted(ctx, &arg_refs, |ctx, vals| {
                    let cb_box = vals[0].clone();
                    let delay_box = vals.get(1).cloned().unwrap_or_else(|| double_literal(0.0));
                    if vals.len() <= 2 {
                        let blk = ctx.block();
                        let cb_handle = unbox_to_i64(blk, &cb_box);
                        let id = blk.call(
                            I64,
                            "js_set_timeout_callback",
                            &[(I64, &cb_handle), (DOUBLE, &delay_box)],
                        );
                        return Ok(nanbox_pointer_inline(blk, &id));
                    }
                    let n = vals.len() - 2;
                    let ptr_reg = fill_arg_buffer(ctx, &vals[2..]);
                    let blk = ctx.block();
                    let cb_handle = unbox_to_i64(blk, &cb_box);
                    let id = blk.call(
                        I64,
                        "js_set_timeout_callback_args",
                        &[
                            (I64, &cb_handle),
                            (DOUBLE, &delay_box),
                            (PTR, &ptr_reg),
                            (I32, &n.to_string()),
                        ],
                    );
                    Ok(nanbox_pointer_inline(blk, &id))
                })?;
                return Ok(Some(boxed));
            }
            "setInterval" if args.len() >= 2 => {
                let arg_refs: Vec<&Expr> = args.iter().collect();
                let boxed = with_operands_rooted(ctx, &arg_refs, |ctx, vals| {
                    let (cb_box, delay_box) = (vals[0].clone(), vals[1].clone());
                    if vals.len() == 2 {
                        let blk = ctx.block();
                        let cb_handle = unbox_to_i64(blk, &cb_box);
                        let id = blk.call(
                            I64,
                            "setInterval",
                            &[(I64, &cb_handle), (DOUBLE, &delay_box)],
                        );
                        return Ok(nanbox_pointer_inline(blk, &id));
                    }
                    let n = vals.len() - 2;
                    let ptr_reg = fill_arg_buffer(ctx, &vals[2..]);
                    let blk = ctx.block();
                    let cb_handle = unbox_to_i64(blk, &cb_box);
                    let id = blk.call(
                        I64,
                        "js_set_interval_callback_args",
                        &[
                            (I64, &cb_handle),
                            (DOUBLE, &delay_box),
                            (PTR, &ptr_reg),
                            (I32, &n.to_string()),
                        ],
                    );
                    Ok(nanbox_pointer_inline(blk, &id))
                })?;
                return Ok(Some(boxed));
            }
            "setImmediate" if !args.is_empty() => {
                // One argument: the callback is consumed by the very next
                // emission, so there is no window to protect. Counted in
                // emissions, not source lines.
                if args.len() == 1 {
                    let cb_box = lower_expr(ctx, &args[0])?;
                    let blk = ctx.block();
                    let cb_handle = unbox_to_i64(blk, &cb_box);
                    let id = blk.call(I64, "js_set_immediate_callback", &[(I64, &cb_handle)]);
                    return Ok(Some(nanbox_pointer_inline(blk, &id)));
                }
                let arg_refs: Vec<&Expr> = args.iter().collect();
                let boxed = with_operands_rooted(ctx, &arg_refs, |ctx, vals| {
                    let cb_box = vals[0].clone();
                    let n = vals.len() - 1;
                    let ptr_reg = fill_arg_buffer(ctx, &vals[1..]);
                    let blk = ctx.block();
                    let cb_handle = unbox_to_i64(blk, &cb_box);
                    let id = blk.call(
                        I64,
                        "js_set_immediate_callback_args",
                        &[(I64, &cb_handle), (PTR, &ptr_reg), (I32, &n.to_string())],
                    );
                    Ok(nanbox_pointer_inline(blk, &id))
                })?;
                return Ok(Some(boxed));
            }
            "clearTimeout" | "clearInterval" | "clearImmediate" if !args.is_empty() => {
                let id_box = lower_expr(ctx, &args[0])?;
                let runtime = match property.as_str() {
                    "clearTimeout" => "js_clear_timeout_value",
                    "clearInterval" => "js_clear_interval_value",
                    _ => "js_clear_immediate_value",
                };
                ctx.block().call_void(runtime, &[(DOUBLE, &id_box)]);
                return Ok(Some(double_literal(f64::from_bits(
                    crate::nanbox::TAG_UNDEFINED,
                ))));
            }
            _ => {}
        }
    }

    if ctx
        .namespace_node_submodules
        .get(ns_name)
        .is_some_and(|submod| submod == "fs/promises")
    {
        // Each of these lowered operand 0 into a bare register and then lowered
        // operands 1 and 2 — arbitrary user code — before the consuming call
        // read it. `fs.writeFile(namePath(), await body(), opts())` is the
        // shape. Routing the whole list through the API closes every
        // operand-to-operand window at once, and costs nothing when the later
        // operands provably cannot collect (`OperandProtection::Reuse`), which
        // is the `writeFile("out.txt", data)` case.
        let fs_promises_helper = match property.as_str() {
            "writeFile" if args.len() >= 2 => Some(("js_fs_promises_write_file", 3usize)),
            "appendFile" if args.len() >= 2 => Some(("js_fs_promises_append_file", 3)),
            "rmdir" => Some(("js_fs_promises_rmdir", 2)),
            _ => None,
        };
        if let Some((helper, arity)) = fs_promises_helper {
            // Only the operands the user actually wrote are lowered; the rest
            // are `undefined` literals, exactly as before.
            let arg_refs: Vec<&Expr> = args.iter().take(arity).collect();
            let promise = with_operands_rooted(ctx, &arg_refs, |ctx, vals| {
                let mut passed: Vec<String> = vals.to_vec();
                passed.resize(arity, undefined_lit());
                let slices: Vec<(crate::types::LlvmType, &str)> =
                    passed.iter().map(|v| (DOUBLE, v.as_str())).collect();
                Ok(ctx.block().call(DOUBLE, helper, &slices))
            })?;
            return Ok(Some(promise));
        }
    }
    // Issue #678 followup (namespace branch): wildcard-namespace
    // import to a V8 module — `import * as R from "ramda";
    // R.sum([1,2,3])`. The V8 module has no static export list
    // and (when no companion Named import is present) nothing
    // seeded `import_function_prefixes` for `property`. Route
    // the member call through the bridge using the
    // namespace's specifier before falling through to the
    // native-prefix lookup. Without this, ramda / date-fns /
    // jose / effect wildcard members fell to the
    // `double_literal(0.0)` stub.
    if let Some(specifier) = ctx.namespace_v8_specifiers.get(ns_name).cloned() {
        return Ok(Some(emit_v8_export_call_rooted(
            ctx, &specifier, property, args,
        )?));
    }
    // Issue #680: prefer the per-namespace map so
    // `random.make` and `tracer.make` resolve to their own
    // sources even when both modules export `make`. Falls
    // back to the flat `import_function_prefixes` for
    // namespaces with no overlapping conflicts.
    let Some(source_prefix) = ctx
        .namespace_member_prefixes
        .get(&(ns_name.clone(), property.clone()))
        .cloned()
        .or_else(|| ctx.import_function_prefixes.get(property).cloned())
    else {
        return Ok(None);
    };
    // Issue #678 followup: if the import lands in a V8-fallback
    // module (e.g. `import * as ink from "ink"` where ink fell
    // back to V8 because yoga-layout pulled in a feature Perry
    // can't compile), route the namespace member through the
    // runtime bridge — no `perry_fn_<src>__<member>` symbol
    // exists for the linker to bind to.
    if let Some(specifier) = ctx.import_function_v8_specifiers.get(property).cloned() {
        return Ok(Some(emit_v8_export_call_rooted(
            ctx, &specifier, property, args,
        )?));
    }
    // Issue #678/#5924: re-exported names (e.g. `export { default as
    // render }`) emit `perry_fn_<src>__default` in the origin —
    // resolve the actual origin suffix before forming the symbol.
    // Namespace-scoped lookup first so a rename in a different namespace
    // imported into this file can't clobber this namespace's unrenamed
    // member of the same name.
    let origin_suffix = crate::expr::import_origin_suffix_ns(
        ctx.import_function_origin_names,
        ctx.namespace_member_origin_names,
        ns_name,
        property,
    );
    let symbol = format!("perry_fn_{}__{}", source_prefix, origin_suffix);
    if ctx.imported_vars.contains(property) {
        // Var-shaped export: fetch closure via zero-arg
        // getter, then closure-call with the user args.
        if args.len() > 16 {
            bail!(
                "perry-codegen: namespace closure call with {} args (max 16)",
                args.len()
            );
        }
        ctx.pending_declares.push((symbol.clone(), DOUBLE, vec![]));
        // The getter runs FIRST and must keep running first: it is the callee
        // reference, and the spec evaluates that before the arguments. Sinking
        // it below them would read a value an argument had reassigned, which is
        // a miscompile rather than a rooting fix. So the closure is the thing
        // that has to survive the window, not the thing that can be re-derived.
        let closure_box = ctx.block().call(DOUBLE, &symbol, &[]);
        let arg_refs: Vec<&Expr> = args.iter().collect();
        // Rooted as `Boxed`, not `Ptr`, and that is a correctness choice rather
        // than a stylistic one. `unbox_to_i64` masks the tag off unconditionally,
        // so on a namespace export that is NOT callable the masked word is not an
        // address at all — pushing it as `Repr::Ptr` would publish garbage into a
        // slot the collector *traces*. Rooting the NaN-boxed value keeps the tag,
        // which is what the scanner reads. The unbox then happens in `finish`,
        // below the window; it is a bitcast and a mask, emits no call, and so
        // cannot itself collect.
        let lowered_args = std::cell::RefCell::new(Vec::<String>::new());
        let result = crate::rooting::with_rooted_accumulator(
            ctx,
            crate::rooting::Repr::Boxed,
            &closure_box,
            crate::rooting::any_operand_may_collect(ctx, args.iter()),
            |ctx, _closure| {
                // The arguments are lowered INSIDE the closure's protected
                // window and rooted against each other by the inner group.
                // `temp_root_truncate` is a stack CUT, and the inner group sits
                // strictly above the closure's own slot, so releasing it leaves
                // the closure rooted for `finish`.
                with_operands_rooted(ctx, &arg_refs, |_ctx, vals| {
                    *lowered_args.borrow_mut() = vals.to_vec();
                    Ok(())
                })
            },
            |ctx, closure_box| {
                // Below every collection point: the inner group's re-reads, then
                // the accumulator's own re-read, then nothing that allocates.
                let lowered = lowered_args.borrow();
                let blk = ctx.block();
                let closure_handle = unbox_to_i64(blk, closure_box);
                let runtime_fn = format!("js_closure_call{}", lowered.len());
                let mut call_args: Vec<(crate::types::LlvmType, &str)> =
                    vec![(I64, &closure_handle)];
                for v in lowered.iter() {
                    call_args.push((DOUBLE, v.as_str()));
                }
                Ok(blk.call(DOUBLE, &runtime_fn, &call_args))
            },
        )?;
        return Ok(Some(result));
    }
    // Function-decl-shaped export: direct call with rest bundling.
    let declared_count = ctx
        .imported_func_param_counts
        .get(property)
        .copied()
        .unwrap_or(args.len());
    let has_rest = ctx.imported_func_has_rest.contains(property);
    if has_rest {
        // #7154's accumulator shape, verbatim: `current` was a raw
        // `*mut ArrayHeader` in a bare SSA register holding the only reference
        // to every argument pushed so far, while the NEXT argument's expression
        // — arbitrary user code — was lowered. The fixed parameters were
        // unprotected across the same push loop.
        //
        // `super::lower_rest_call_args_rooted` was written for exactly this and
        // this path never adopted it. Delegating is the same posture as calling
        // `lower_expr`: the helper's contract is documented and audited, and no
        // ordering decision is made here. It also pads the fixed parameters to
        // the declared arity, which the hand-rolled loop did not — a call with
        // fewer arguments than the callee declares used to emit a call of the
        // wrong arity.
        let fixed_count = declared_count.saturating_sub(1);
        let (lowered, guard) = super::lower_rest_call_args_rooted(
            ctx,
            args,
            fixed_count,
            &[super::RestBundle {
                from: fixed_count,
                mark_arguments_object: false,
            }],
        )?;
        let arg_types: Vec<crate::types::LlvmType> =
            std::iter::repeat_n(DOUBLE, lowered.len()).collect();
        ctx.pending_declares
            .push((symbol.clone(), DOUBLE, arg_types));
        return Ok(Some(super::emit_rooted_call(ctx, &symbol, &lowered, guard)));
    }
    // The plain argument loop had no protection at all — #7240's shape, in a
    // path that post-dates the fix. Zero cost when nothing in the list can
    // collect.
    let arg_refs: Vec<&Expr> = args.iter().collect();
    let call = with_operands_rooted(ctx, &arg_refs, |ctx, vals| {
        let mut lowered: Vec<String> = vals.to_vec();
        // Pad missing trailing args with TAG_UNDEFINED.
        lowered.resize(lowered.len().max(declared_count), undefined_lit());
        let arg_types: Vec<crate::types::LlvmType> =
            std::iter::repeat_n(DOUBLE, lowered.len()).collect();
        ctx.pending_declares
            .push((symbol.clone(), DOUBLE, arg_types));
        let arg_slices: Vec<(crate::types::LlvmType, &str)> =
            lowered.iter().map(|s| (DOUBLE, s.as_str())).collect();
        Ok(ctx.block().call(DOUBLE, &symbol, &arg_slices))
    })?;
    Ok(Some(call))
}

/// Lower a V8-bridge export call's arguments with each one rooted across the
/// evaluation of the ones that follow, then emit the bridge call.
///
/// Both bridge arms used a bare `for a in args { lowered.push(lower_expr(a)?) }`
/// — argument 0 sits in an SSA register while arguments 1..n run arbitrary user
/// code, and `emit_v8_export_call` marshals them all afterwards. That is #7240's
/// shape; the two arms differ only in which map resolved the specifier, so the
/// fix is one function rather than two edits.
fn emit_v8_export_call_rooted(
    ctx: &mut FnCtx<'_>,
    specifier: &str,
    property: &str,
    args: &[Expr],
) -> Result<String> {
    let arg_refs: Vec<&Expr> = args.iter().collect();
    with_operands_rooted(ctx, &arg_refs, |ctx, vals| {
        Ok(crate::expr::emit_v8_export_call(
            ctx, specifier, property, vals,
        ))
    })
}
