//! Issue #636: namespace member call —
//! `Call { callee: PropertyGet { ExternFuncRef(ns), method }, args }`
//! where `ns ∈ namespace_imports`.

use anyhow::{bail, Result};
use perry_hir::Expr;

use crate::expr::{lower_expr, nanbox_pointer_inline, unbox_to_i64, FnCtx};
use crate::nanbox::double_literal;
use crate::types::{DOUBLE, I32, I64};

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
    let Expr::PropertyGet { object, property } = callee else {
        return Ok(None);
    };
    let Expr::ExternFuncRef { name: ns_name, .. } = object.as_ref() else {
        return Ok(None);
    };
    if !ctx.namespace_imports.contains(ns_name) {
        return Ok(None);
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
        let mut lowered: Vec<String> = Vec::with_capacity(args.len());
        for a in args {
            lowered.push(lower_expr(ctx, a)?);
        }
        return Ok(Some(crate::expr::emit_v8_export_call(
            ctx, &specifier, property, &lowered,
        )));
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
        let mut lowered: Vec<String> = Vec::with_capacity(args.len());
        for a in args {
            lowered.push(lower_expr(ctx, a)?);
        }
        return Ok(Some(crate::expr::emit_v8_export_call(
            ctx, &specifier, property, &lowered,
        )));
    }
    // Issue #678: re-exported names (e.g. `export { default as
    // render }`) emit `perry_fn_<src>__default` in the origin —
    // resolve the actual origin suffix before forming the symbol.
    let origin_suffix =
        crate::expr::import_origin_suffix(ctx.import_function_origin_names, property);
    let symbol = format!("perry_fn_{}__{}", source_prefix, origin_suffix);
    if ctx.imported_vars.contains(property) {
        // Var-shaped export: fetch closure via zero-arg
        // getter, then closure-call with the user args.
        ctx.pending_declares.push((symbol.clone(), DOUBLE, vec![]));
        let closure_box = ctx.block().call(DOUBLE, &symbol, &[]);
        let mut lowered: Vec<String> = Vec::with_capacity(args.len());
        for a in args {
            lowered.push(lower_expr(ctx, a)?);
        }
        if lowered.len() > 16 {
            bail!(
                "perry-codegen: namespace closure call with {} args (max 16)",
                lowered.len()
            );
        }
        let blk = ctx.block();
        let closure_handle = unbox_to_i64(blk, &closure_box);
        let runtime_fn = format!("js_closure_call{}", lowered.len());
        let mut call_args: Vec<(crate::types::LlvmType, &str)> = vec![(I64, &closure_handle)];
        for v in &lowered {
            call_args.push((DOUBLE, v.as_str()));
        }
        return Ok(Some(blk.call(DOUBLE, &runtime_fn, &call_args)));
    }
    // Function-decl-shaped export: direct call with rest bundling.
    let declared_count = ctx
        .imported_func_param_counts
        .get(property)
        .copied()
        .unwrap_or(args.len());
    let has_rest = ctx.imported_func_has_rest.contains(property);
    let mut lowered: Vec<String> = Vec::with_capacity(declared_count);
    if has_rest {
        let fixed_count = declared_count.saturating_sub(1);
        for a in args.iter().take(fixed_count) {
            lowered.push(lower_expr(ctx, a)?);
        }
        let rest_count = args.len().saturating_sub(fixed_count);
        let cap = (rest_count as u32).to_string();
        let mut current = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
        for a in args.iter().skip(fixed_count) {
            let v = lower_expr(ctx, a)?;
            let blk = ctx.block();
            current = blk.call(I64, "js_array_push_f64", &[(I64, &current), (DOUBLE, &v)]);
        }
        let rest_box = nanbox_pointer_inline(ctx.block(), &current);
        lowered.push(rest_box);
    } else {
        for a in args {
            lowered.push(lower_expr(ctx, a)?);
        }
        // Pad missing trailing args with TAG_UNDEFINED.
        let undef_lit = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
        while lowered.len() < declared_count {
            lowered.push(undef_lit.clone());
        }
    }
    let arg_types: Vec<crate::types::LlvmType> =
        std::iter::repeat(DOUBLE).take(lowered.len()).collect();
    ctx.pending_declares
        .push((symbol.clone(), DOUBLE, arg_types));
    let arg_slices: Vec<(crate::types::LlvmType, &str)> =
        lowered.iter().map(|s| (DOUBLE, s.as_str())).collect();
    Ok(Some(ctx.block().call(DOUBLE, &symbol, &arg_slices)))
}
