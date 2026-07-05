//! AST `Expr` → HIR `Expr` lowering: `lower_expr`, `lower_expr_assignment`,
//! and the `Text` reactive-template desugar helper.
//!
//! Extracted from `lower/mod.rs` so the entry-point file stays under the
//! 2,000-LOC soft cap. The `match expr` body inside `lower_expr` still
//! delegates the larger variant arms to existing sibling modules
//! (`expr_call`, `expr_member`, `expr_assign`, `expr_function`,
//! `expr_object`, `expr_new`, `expr_misc`); this file holds only the
//! `match` skeleton, delegating the larger inline arms (`Ident`, `Bin`,
//! `Unary`, `OptChain`, `Class`) and the smaller helpers to its own
//! sibling modules under `lower_expr/`.
//!
//! Visibility note: `lower_expr_assignment` and `try_desugar_reactive_text`
//! were `pub(super)` — bumped to `pub(crate)` so the mod.rs named
//! re-exports can propagate them.

use anyhow::{anyhow, Result};
use perry_types::{LocalId, Type};
use swc_common::Spanned;
use swc_ecma_ast as ast;

use super::*;
use crate::ir::*;
use crate::lower_types::extract_ts_type_with_ctx;

// Sibling modules holding the extracted helpers and large match arms.
mod arm_bin;
mod arm_class;
mod arm_ident;
mod arm_optchain;
mod arm_unary;
mod assignment;
mod helpers;
mod reactive_text;

pub(crate) use arm_bin::lower_bin_expr;
pub(crate) use arm_class::lower_class_expr;
pub(crate) use arm_ident::lower_ident_expr;
pub(crate) use arm_optchain::lower_opt_chain_expr;
pub(crate) use arm_unary::lower_unary_expr;
pub(crate) use assignment::lower_expr_assignment;
pub(crate) use helpers::{
    anonymous_class_has_static_name_member, expr_uses_stack_heavy_chain_lowering,
    global_script_this_enabled, is_cjs_style_native_default_import, is_fetch_global_value_name,
    is_known_global_identifier_name, lower_expr_with_json_parse_type_hint,
    native_module_binding_value, opt_call_func_nullish_guard, opt_call_receiver_repeatable,
    relower_trace, strict_global_assign_existing_or_throw, throw_reference_error_expr,
    typed_parse_codegen_supports, with_implicit_unset_let, with_set_fallback_for_ident,
    wrap_with_gets,
};
pub(crate) use reactive_text::try_desugar_reactive_text;

/// Maximum overall `lower_expr` recursion depth before lowering bails with a
/// diagnostic instead of overflowing the native stack (#5259).
///
/// Object-literal lowering intentionally supports very deep nested shapes
/// (see `nested_object_literal_lowers_in_linear_time`). Keep this broad cap
/// high enough for those fixtures; stack-heavy chain forms are guarded by the
/// lower `MAX_EXPR_CHAIN_LOWER_DEPTH` limit below.
pub(crate) const MAX_EXPR_LOWER_DEPTH: u32 = 8192;

/// Stack-heavy expression chains (`1+1+…`, `o.a.a.…`, `a||a||…`) recurse with
/// larger lowerer frames than object literals. This lower shape-specific cap
/// converts degenerate chain input into a diagnostic before debug/CI stacks can
/// overflow, without rejecting supported deep object-literal fixtures.
pub(crate) const MAX_EXPR_CHAIN_LOWER_DEPTH: u32 = 512;

const EXPR_LOWER_STACK_RED_ZONE: usize = 256 * 1024;
const EXPR_LOWER_STACK_SEGMENT: usize = 2 * 1024 * 1024;

pub(crate) fn lower_expr(ctx: &mut LoweringContext, expr: &ast::Expr) -> Result<Expr> {
    if relower_trace::enabled() {
        let sp = expr.span();
        relower_trace::record(sp.lo.0, sp.hi.0);
    }
    // #5259: guard the recursive descent. Without this, a pathologically
    // nested expression (`1+1+…`, `o.a.a.…`, `a||a||…`) overflows the native
    // stack and SIGABRTs with no diagnostic. The depth counter turns that into
    // a clean "nested too deeply" error. It is decremented on every exit path,
    // including the error returns inside `lower_expr_impl`, so a recoverable
    // lowering error elsewhere doesn't leave the depth permanently inflated.
    ctx.expr_lower_depth += 1;
    let max_depth = if expr_uses_stack_heavy_chain_lowering(expr) {
        MAX_EXPR_CHAIN_LOWER_DEPTH
    } else {
        MAX_EXPR_LOWER_DEPTH
    };
    if ctx.expr_lower_depth > max_depth {
        ctx.expr_lower_depth -= 1;
        crate::lower_bail!(
            expr.span(),
            "expression nested too deeply (exceeded {} levels); split the \
             chain across statements or intermediate variables",
            max_depth
        );
    }
    let result = stacker::maybe_grow(EXPR_LOWER_STACK_RED_ZONE, EXPR_LOWER_STACK_SEGMENT, || {
        lower_expr_impl(ctx, expr)
    });
    ctx.expr_lower_depth -= 1;
    result
}

fn lower_expr_impl(ctx: &mut LoweringContext, expr: &ast::Expr) -> Result<Expr> {
    match expr {
        ast::Expr::Lit(lit) => lower_lit(lit),
        ast::Expr::Ident(ident) => lower_ident_expr(ctx, ident),
        ast::Expr::Bin(bin) => lower_bin_expr(ctx, bin),
        ast::Expr::Unary(unary) => lower_unary_expr(ctx, unary),
        ast::Expr::Call(call) => expr_call::lower_call(ctx, call),
        ast::Expr::Member(member) => expr_member::lower_member(ctx, member),
        ast::Expr::Paren(paren) => lower_expr(ctx, &paren.expr),
        ast::Expr::Assign(assign) => expr_assign::lower_assign(ctx, assign),
        ast::Expr::Cond(cond) => expr_misc::lower_cond(ctx, cond),
        ast::Expr::Array(array) => {
            // Check if any elements need the spread-aware representation.
            let has_spread = array
                .elems
                .iter()
                .filter_map(|elem| elem.as_ref())
                .any(|elem| elem.spread.is_some());
            let has_hole = array.elems.iter().any(|elem| elem.is_none());

            if has_spread || has_hole {
                // Use ArraySpread for arrays with spread elements or elisions.
                // Elisions must remain holes, not explicit undefined values:
                // own-property checks and iteration observe the difference.
                let elements = array
                    .elems
                    .iter()
                    .map(|elem| {
                        let Some(elem) = elem.as_ref() else {
                            return Ok(ArrayElement::Hole);
                        };
                        let expr = lower_expr(ctx, &elem.expr)?;
                        if elem.spread.is_some() {
                            if is_generator_call_expr(ctx, &expr) {
                                Ok(ArrayElement::Spread(Expr::IteratorToArray(Box::new(expr))))
                            } else {
                                Ok(ArrayElement::Spread(expr))
                            }
                        } else {
                            Ok(ArrayElement::Expr(expr))
                        }
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok(Expr::ArraySpread(elements))
            } else {
                let elements = array
                    .elems
                    .iter()
                    .map(|elem| lower_expr(ctx, &elem.as_ref().unwrap().expr))
                    .collect::<Result<Vec<_>>>()?;
                Ok(Expr::Array(elements))
            }
        }
        ast::Expr::Object(obj) => expr_object::lower_object(ctx, obj),
        ast::Expr::This(_) => {
            // Module TOP-LEVEL `this` is Node-CJS `module.exports` — a fresh
            // plain object, not `globalThis` (the default `node
            // --experimental-strip-types` parity oracle runs files as
            // CommonJS, where top-level `this === module.exports === {}`).
            // Function/class/with bodies keep dynamic `Expr::This` semantics,
            // handled by codegen's ThisContext.
            if ctx.scope_depth == 0
                && ctx.current_class.is_none()
                && ctx.with_env_stack.is_empty()
                && !ctx.is_external_module
            {
                // Global-script mode (`PERRY_GLOBAL_SCRIPT_THIS`): a conforming
                // Test262 host evaluates each assembled case as a *global
                // script* (the Node oracle does this via
                // `vm.runInThisContext`, #5346/#5511), where top-level `this`
                // is `globalThis`, not a CJS exports object. Opt-in only: the
                // default stays CJS-`{}` so standalone builds still match
                // `node --experimental-strip-types`. (#5579 global prop-desc
                // cluster — `verifyProperty(this, "decodeURI", ...)`.)
                if global_script_this_enabled() {
                    // #5833: a top-level `this` read is just as much a global-
                    // object reference as the literal `globalThis` token — see
                    // `saw_global_this_expr`'s doc comment.
                    ctx.saw_global_this_expr = true;
                    return Ok(Expr::GlobalThisExpr);
                }
                return Ok(Expr::ModuleTopThis);
            }
            Ok(Expr::This)
        }
        ast::Expr::New(new_expr) => expr_new::lower_new(ctx, new_expr),
        ast::Expr::Arrow(arrow) => expr_function::lower_arrow(ctx, arrow),
        ast::Expr::Fn(fn_expr) => expr_function::lower_fn_expr(ctx, fn_expr),
        ast::Expr::Await(await_expr) => expr_misc::lower_await(ctx, await_expr),
        ast::Expr::SuperProp(super_prop) => expr_misc::lower_super_prop(ctx, super_prop),
        ast::Expr::Update(update) => expr_misc::lower_update(ctx, update),
        ast::Expr::Tpl(tpl) => expr_misc::lower_tpl(ctx, tpl),
        ast::Expr::OptChain(opt_chain) => lower_opt_chain_expr(ctx, opt_chain),
        ast::Expr::TsAs(ts_as) => {
            // TypeScript 'as' type assertion - at runtime, just evaluate the expression
            // The type assertion is compile-time only
            lower_expr_with_json_parse_type_hint(ctx, &ts_as.expr, &ts_as.type_ann)
        }
        ast::Expr::TsNonNull(ts_non_null) => {
            // TypeScript non-null assertion (value!) - at runtime, just the expression
            lower_expr(ctx, &ts_non_null.expr)
        }
        ast::Expr::TsTypeAssertion(ts_assertion) => {
            // TypeScript angle-bracket type assertion (<Type>value) - same as 'as', compile-time only
            lower_expr_with_json_parse_type_hint(ctx, &ts_assertion.expr, &ts_assertion.type_ann)
        }
        ast::Expr::TsConstAssertion(ts_const) => {
            // TypeScript 'as const' assertion - at runtime, just evaluate the expression
            // The const assertion only affects type inference, not runtime behavior
            lower_expr(ctx, &ts_const.expr)
        }
        ast::Expr::TsSatisfies(ts_satisfies) => {
            // TypeScript 'satisfies' operator - compile-time type check only
            lower_expr(ctx, &ts_satisfies.expr)
        }
        ast::Expr::TsInstantiation(ts_inst) => {
            // TypeScript generic instantiation (func<Type>) - at runtime, just the expression
            lower_expr(ctx, &ts_inst.expr)
        }
        ast::Expr::Seq(seq) => expr_misc::lower_seq(ctx, seq),
        ast::Expr::MetaProp(meta_prop) => expr_misc::lower_meta_prop(ctx, meta_prop),
        ast::Expr::Yield(y) => expr_misc::lower_yield(ctx, y),
        ast::Expr::TaggedTpl(tagged) => {
            // Tagged template literals: tag`Hello ${name},${42}!`
            // Two cases:
            //  (a) String.raw — kept as a fast-path string concatenation that
            //      preserves backslashes literally (no escape processing).
            //  (b) Any other tag function — desugar to a regular function call:
            //      tag(["Hello ", ",", "!"], name, 42)
            //      i.e. first arg is the array of cooked string literal parts,
            //      followed by each interpolated value as its own argument.
            //      The matches the JS spec for `tag` callbacks (sans `.raw`).
            let is_string_raw = match &*tagged.tag {
                ast::Expr::Member(member) => {
                    let obj_is_string = match &member.obj.as_ref() {
                        ast::Expr::Ident(id) => id.sym.as_ref() == "String",
                        _ => false,
                    };
                    let prop_is_raw = match &member.prop {
                        ast::MemberProp::Ident(id) => id.sym.as_ref() == "raw",
                        _ => false,
                    };
                    obj_is_string && prop_is_raw
                }
                _ => false,
            };

            let tpl = &*tagged.tpl;
            if tpl.quasis.is_empty() {
                return Ok(Expr::String(String::new()));
            }

            if is_string_raw {
                // Fast path: build string via direct concatenation using `raw` text
                let first_raw = tpl.quasis.first().map(|q| q.raw.as_ref()).unwrap_or("");
                let mut result = Expr::String(first_raw.to_string());

                for (i, expr) in tpl.exprs.iter().enumerate() {
                    let lowered = lower_expr(ctx, expr)?;
                    result = Expr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(result),
                        right: Box::new(lowered),
                    };

                    if let Some(quasi) = tpl.quasis.get(i + 1) {
                        let quasi_str: &str = quasi.raw.as_ref();
                        if !quasi_str.is_empty() {
                            result = Expr::Binary {
                                op: BinaryOp::Add,
                                left: Box::new(result),
                                right: Box::new(Expr::String(quasi_str.to_string())),
                            };
                        }
                    }
                }

                return Ok(result);
            }

            // General case: desugar to `tag(stringsArray, ...exprs)`. The
            // strings array carries the cooked text (escapes processed) AS
            // the array elements AND the raw text (escapes preserved) via
            // a thread-local side table populated at the call site —
            // `TaggedTemplateStrings` codegen emits both arrays, then asks
            // the runtime for the cached frozen template object so
            // `strings.raw` reads can resolve via the matching
            // `Expr::TemplateRaw` fold below.
            let cooked_strings: Vec<Expr> = tpl
                .quasis
                .iter()
                .map(|q| {
                    let cooked_owned: Option<String> = q
                        .cooked
                        .as_ref()
                        .and_then(|c| c.as_str().map(|s| s.to_string()));
                    let s = cooked_owned.unwrap_or_else(|| q.raw.as_ref().to_string());
                    Expr::String(s)
                })
                .collect();
            let raw_strings: Vec<String> = tpl
                .quasis
                .iter()
                .map(|q| q.raw.as_ref().to_string())
                .collect();
            let strings_array = Expr::TaggedTemplateStrings {
                site_id: ctx.fresh_tagged_template_site_id(),
                cooked: cooked_strings,
                raw: raw_strings,
            };

            let mut call_args: Vec<Expr> = Vec::with_capacity(tpl.exprs.len() + 1);
            call_args.push(strings_array);
            for e in &tpl.exprs {
                call_args.push(lower_expr(ctx, e)?);
            }

            let callee = lower_expr(ctx, &tagged.tag)?;
            Ok(Expr::Call {
                callee: Box::new(callee),
                args: call_args,
                type_args: vec![],
                byte_offset: 0,
            })
        }
        // Class expression used as a value (not in `new` context) —
        // refs #740. JS semantics: a class expression evaluates to the
        // class constructor itself. Previously we emitted an empty `new`
        // here, which bound the local to a zero-arg instance instead of
        // the class — so `const C = class { ... }; new C(args)` ran the
        // ctor with no args, and `O.Inner` inside an object literal held
        // a stillborn instance instead of a constructor. Lower to a
        // `ClassRef` so the constructor identity survives the value path
        // and `new` site rerouting (via `local_class_aliases`) picks it
        // back up.
        ast::Expr::Class(class_expr) => lower_class_expr(ctx, class_expr),
        ast::Expr::JSXElement(jsx) => lower_jsx_element(ctx, jsx),
        ast::Expr::JSXFragment(jsx) => lower_jsx_fragment(ctx, jsx),
        _ => Err(anyhow!("Unsupported expression type: {:?}", expr)),
    }
}
