//! Constructor-argument marshaling for `new` lowering (#6537 size-gate split
//! from `new.rs` — pure relocation, no behavior change).
//!
//! Holds the inline-ctor param binding/restore scope, the user-arg vs
//! synthesized `__perry_cap_<id>` tail split (`CaptureFill`,
//! `inline_constructor_param_values_with_class`,
//! `new_site_args_carry_appended_caps` — #6530), rest/`arguments` packing,
//! imported-ctor arg marshaling, and the standalone
//! `<class>_constructor`-symbol call path.

use anyhow::Result;
use perry_hir::{Expr, Param};
use perry_types::Type as HirType;

use super::new_helpers::effective_constructor_param_count;
use crate::expr::{lower_expr, nanbox_pointer_inline, FnCtx};
use crate::nanbox::double_literal;
use crate::types::{DOUBLE, I32, I64};

pub(crate) struct InlineConstructorScope {
    locals: std::collections::HashMap<u32, String>,
    local_types: std::collections::HashMap<u32, HirType>,
    boxed_vars: std::collections::HashSet<u32>,
}

pub(crate) fn restore_inline_constructor_scope(ctx: &mut FnCtx<'_>, saved: InlineConstructorScope) {
    ctx.locals = saved.locals;
    ctx.local_types = saved.local_types;
    ctx.boxed_vars = saved.boxed_vars;
}

pub(crate) fn bind_inline_constructor_params(
    ctx: &mut FnCtx<'_>,
    params: &[Param],
    lowered_args: &[String],
    capture_fill: Option<CaptureFill>,
) -> InlineConstructorScope {
    let saved = InlineConstructorScope {
        locals: ctx.locals.clone(),
        local_types: ctx.local_types.clone(),
        boxed_vars: ctx.boxed_vars.clone(),
    };

    crate::codegen::arguments::add_arguments_mapped_boxes(params, &mut ctx.boxed_vars);
    let values =
        inline_constructor_param_values_with_class(ctx, params, lowered_args, capture_fill);
    for (param, arg_val) in params.iter().zip(values.iter()) {
        let boxed_param = ctx.boxed_vars.contains(&param.id) && param.arguments_object.is_none();
        let slot = ctx
            .func
            .alloca_entry(if boxed_param { I64 } else { DOUBLE });
        if boxed_param {
            let arg_bits = ctx.block().bitcast_double_to_i64(arg_val);
            let box_ptr = ctx
                .block()
                .call(I64, "js_box_alloc_bits", &[(I64, &arg_bits)]);
            ctx.block().store(I64, &box_ptr, &slot);
        } else {
            ctx.block().store(DOUBLE, arg_val, &slot);
        }
        ctx.locals.insert(param.id, slot);
        ctx.local_types.insert(param.id, param.ty.clone());
    }

    crate::codegen::arguments::materialize_arguments_object(
        ctx,
        params,
        crate::codegen::arguments::ArgumentsCallee::Undefined,
    );

    saved
}

/// Where a synthesized `__perry_cap_<id>` param's value comes from when the
/// `new` site did not supply it as an appended arg.
#[derive(Clone, Copy)]
pub(crate) struct CaptureFill {
    /// The constructing class's id, used to read its DECL-SITE capture
    /// snapshot (`js_class_capture_value(cid, slot)`).
    pub(crate) cid: u32,
    /// `true` when `lowered_args` does NOT contain appended cap values — the
    /// member-callee `new ns.C(...)` path. Then ALL `lowered_args` are user
    /// args and EVERY cap param fills from the snapshot. `false` for the
    /// bare-identifier `new C(...)` path, where the HIR appended the caps as
    /// trailing args (tail-split keeps binding them); the snapshot then only
    /// backfills a cap the HIR didn't append.
    pub(crate) caps_absent_from_args: bool,
}

impl CaptureFill {
    /// Snapshot-only BACKFILL for a cap param the caller's args did not
    /// supply: the `lowered_args` still carry their appended cap values
    /// (tail-split keeps binding them). Used by the `super(...)` inline path,
    /// which explicitly forwards parent caps as args.
    pub(crate) fn backfill(cid: u32) -> Self {
        CaptureFill {
            cid,
            caps_absent_from_args: false,
        }
    }
}

/// As [`inline_constructor_param_values`], but fills a synthesized
/// `__perry_cap_<id>` param that the `new` site did not supply from the
/// class's DECL-SITE capture snapshot (`js_class_capture_value(cid, slot)`)
/// instead of `undefined`.
///
/// #5437 (W6): a member-callee construct `new ns.C()` of a function-nested
/// class that captured an enclosing local is statically routed to
/// `lower_new("C", [])` (the `#740` object-field-alias arm in
/// `expr/new_dynamic.rs`) — the captures are NOT appended as trailing args
/// (that only happens for the bare-identifier `new C()` HIR arm). With no
/// cap args the cap params bound to `undefined` and every method reading a
/// captured local saw `undefined`. The bare-`new C()` HIR-append cannot be
/// reused for `new ns.C()`: at the outer (member) `new` site the captured
/// enclosing local is OUT OF SCOPE, so `LocalGet(cid)` would itself read
/// `undefined`. The decl-site snapshot (registered at the class's
/// declaration by `js_class_register_capture_values`) holds the correct
/// captured values. `fill = None` keeps the prior `undefined` fill (no
/// behavior change for non-capturing/unknown classes).
fn inline_constructor_param_values_with_class(
    ctx: &mut FnCtx<'_>,
    params: &[Param],
    lowered_args: &[String],
    capture_fill: Option<CaptureFill>,
) -> Vec<String> {
    let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
    // Synthesized `__perry_cap_<id>` capture params are always TRAILING
    // params, and `Expr::New` sites always append the capture values after
    // the user args — but the two sides need not agree on the USER arity.
    // A no-user-ctor capturing class has zero user params while the `new`
    // site may pass user args (`new ZodString({})` — the vendored-zod
    // bundle), so positional binding put the user arg into the capture
    // slot. Bind capture params from the args TAIL and user params from
    // the head.
    //
    // #5437: when a decl-site snapshot is available (`capture_fill = Some`),
    // EVERY synthesized cap param is filled from that snapshot — the
    // authoritative decl-site capture value — regardless of whether the `new`
    // site appended cap args. This is what closes W6: the bundle's bare-`new
    // uS(...)` appends a MIS-BOXED `uw` cap (the multi-level capture chain
    // materialized the wrong value), while `uS`'s decl-site snapshot holds the
    // correct module-exports object; preferring the snapshot makes
    // `new uw.SharedCacheControls` resolve.
    //
    // The split between user args and the (now ignored) appended cap args
    // still matters for the USER params:
    //   - member-callee `new ns.C(...)`: caps are NOT appended, so ALL
    //     `lowered_args` are user args → `n_caps = 0`. (`new ns.C("ARG")`
    //     binds the user param to `"ARG"`, the cap to the snapshot.)
    //   - bare-identifier `new C(...)`: the HIR appended the caps as trailing
    //     args, so strip them (tail-split) to recover the leading user args;
    //     the stripped cap values are discarded in favour of the snapshot.
    let caps_absent = matches!(
        capture_fill,
        Some(CaptureFill {
            caps_absent_from_args: true,
            ..
        })
    );
    let n_caps = if caps_absent {
        0
    } else {
        params
            .iter()
            .filter(|p| {
                p.name.starts_with("__perry_cap_") && !p.is_rest && p.arguments_object.is_none()
            })
            .count()
            .min(lowered_args.len())
    };
    let user_len = lowered_args.len() - n_caps;
    let (user_args, cap_args) = lowered_args.split_at(user_len);
    let mut cap_iter = cap_args.iter();

    let mut out = Vec::with_capacity(params.len());
    let mut visible_index = 0usize;
    // The cap-slot index of the NEXT cap param: the index of the value in
    // the decl-site snapshot (registered in `captures_vec` / cap-param
    // declaration order, which is the same order they appear here).
    let mut cap_slot = 0u32;
    for param in params {
        if param.name.starts_with("__perry_cap_")
            && !param.is_rest
            && param.arguments_object.is_none()
        {
            let slot = cap_slot;
            cap_slot += 1;
            // Consume the appended cap arg so the tail stays aligned. When a
            // decl-site snapshot is registered for `cid`, it is authoritative
            // (W6: the appended arg may be a mis-boxed multi-level capture);
            // otherwise (e.g. an inline anonymous class capturing a
            // `require(...)`-derived local — #5437 OTel `trace`) NO snapshot is
            // registered, so fall back to the appended cap arg rather than
            // dropping it to `undefined`.
            let appended = cap_iter.next();
            out.push(match capture_fill {
                Some(CaptureFill { cid, .. }) => {
                    let fallback = appended.cloned().unwrap_or_else(|| undef.clone());
                    ctx.block().call(
                        DOUBLE,
                        "js_class_capture_value_or",
                        &[
                            (I32, &cid.to_string()),
                            (I32, &slot.to_string()),
                            (DOUBLE, &fallback),
                        ],
                    )
                }
                None => appended.cloned().unwrap_or_else(|| undef.clone()),
            });
        } else if param.arguments_object.is_some() {
            out.push(pack_lowered_args_array(ctx, user_args));
        } else if param.is_rest {
            let tail = if visible_index < user_args.len() {
                &user_args[visible_index..]
            } else {
                &[]
            };
            out.push(pack_lowered_args_array(ctx, tail));
        } else {
            out.push(
                user_args
                    .get(visible_index)
                    .cloned()
                    .unwrap_or_else(|| undef.clone()),
            );
            visible_index += 1;
        }
    }
    out
}

/// #6530: true when the trailing args of a bare-identifier `new C(...)` site
/// are the HIR-appended capture forwards for `class`'s synthesized
/// `__perry_cap_<id>` constructor params. The HIR `Expr::New` arm appends
/// `LocalGet(cid)` per captured id, in cap-param order, ONLY where those
/// locals are in scope (the class's declaring function) — so each trailing
/// arg must be a `LocalGet` whose id equals the id embedded in the matching
/// param name. Any mismatch (a sibling-class method's `new ZodEffects({...})`
/// carries only user args) means the caps are absent and the tail-split must
/// not steal user args as cap fallbacks.
///
/// Soundness of the id match: `LocalId`s come from a single MODULE-WIDE
/// counter (`LoweringContext::fresh_local` — never reset per function), so
/// `LocalGet(id)` anywhere in the module denotes the one local with that id.
/// A user expression can therefore only produce the cap-matching ids (all of
/// them, in declaration order) by referencing the captured locals themselves
/// — possible only in scopes where they are visible, which are exactly the
/// scopes where the HIR appends the caps anyway (and there the appended tail
/// follows the user args, so the tail-split still binds correctly).
pub(super) fn new_site_args_carry_appended_caps(class: &perry_hir::Class, args: &[Expr]) -> bool {
    let Some(ctor) = class.constructor.as_ref() else {
        return false;
    };
    let cap_params: Vec<&Param> = ctor
        .params
        .iter()
        .filter(|p| {
            p.name.starts_with("__perry_cap_") && !p.is_rest && p.arguments_object.is_none()
        })
        .collect();
    if cap_params.is_empty() || args.len() < cap_params.len() {
        return false;
    }
    let tail = &args[args.len() - cap_params.len()..];
    tail.iter().zip(cap_params.iter()).all(|(arg, p)| {
        matches!(arg, Expr::LocalGet(id)
            if perry_hir::cap_fields::cap_field_outer_id(&p.name) == Some(*id))
    })
}

fn pack_lowered_args_array(ctx: &mut FnCtx<'_>, args: &[String]) -> String {
    let cap = (args.len() as u32).to_string();
    let mut current = ctx.block().call(I64, "js_array_alloc", &[(I32, &cap)]);
    for value in args {
        current = ctx.block().call(
            I64,
            "js_array_push_f64",
            &[(I64, &current), (DOUBLE, value.as_str())],
        );
    }
    nanbox_pointer_inline(ctx.block(), &current)
}

pub(super) fn lower_constructor_arg(ctx: &mut FnCtx<'_>, arg: &Expr) -> Result<String> {
    let prev_discard = ctx.discard_expr_value;
    ctx.discard_expr_value = false;
    let lowered = lower_expr(ctx, arg);
    ctx.discard_expr_value = prev_discard;
    lowered
}

/// Marshal the lowered `new`-site args into the value list a cross-module
/// imported constructor symbol expects. The source module compiled the
/// standalone `<class>_constructor(this, p0, …)` with `ctor.param_count`
/// explicit slots. When the constructor's last param is `...rest`
/// (`ctor.has_rest`), that final slot must receive a PACKED ARRAY of every
/// trailing arg — not the first trailing arg passed raw. Mirrors the
/// inline-ctor `inline_constructor_param_values` rest packing and the
/// `method_has_rest` path for imported methods (#672). Returns exactly
/// `ctor.param_count` value strings; missing leading args are padded with
/// `undefined`.
pub(super) fn marshal_imported_ctor_args(
    ctx: &mut FnCtx<'_>,
    ctor: &crate::codegen::ImportedCtor,
    lowered_args: &[String],
) -> Vec<String> {
    let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
    let param_count = ctor.param_count;
    if ctor.has_rest && param_count > 0 {
        // The first `param_count - 1` slots are positional; the last slot is
        // the rest array packing every remaining arg.
        let n_positional = param_count - 1;
        let mut out: Vec<String> = Vec::with_capacity(param_count);
        for i in 0..n_positional {
            out.push(
                lowered_args
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| undef.clone()),
            );
        }
        let tail: Vec<String> = lowered_args.iter().skip(n_positional).cloned().collect();
        out.push(pack_lowered_args_array(ctx, &tail));
        out
    } else {
        // No rest: positional, padded to `param_count` with `undefined`.
        let mut out: Vec<String> = lowered_args.to_vec();
        while out.len() < param_count {
            out.push(undef.clone());
        }
        // #6537 review: `param_count.max(out.len())` made this a no-op, so a
        // call site passing MORE args than the imported ctor's fixed arity
        // emitted excess operands — violating the documented "returns exactly
        // `ctor.param_count`" contract (the compiled `<class>_constructor`
        // symbol has exactly that many post-`this` params; JS ignores extra
        // ctor args). Truncate for real.
        out.truncate(param_count);
        out
    }
}

/// The effective constructor arity for `new <class>(...)`: the class's own
/// ctor params, else — for a subclass with no own ctor — the closest
/// ancestor-with-a-ctor's param count (the synthesized default ctor forwards
/// `super(...args)`). Matches the standalone-ctor signature emitted in
/// `codegen/artifacts.rs`, so callers pass the right number of args.

/// Emit a call to the shared standalone `<class>_constructor` symbol and
/// return the raw value it produced. The standalone ctor function returns
/// `undefined` for an ordinary constructor (implicit `return this`) or the
/// explicitly-returned value for a `return <expr>` body — the caller applies
/// `js_ctor_return_override` to that raw value to honor ECMAScript's
/// constructor-return-override rule (a returned object/function replaces the
/// freshly-allocated `this`). Returns `None` when no standalone symbol exists.
pub(super) fn call_local_constructor_symbol(
    ctx: &mut FnCtx<'_>,
    class: &perry_hir::Class,
    obj_box: &str,
    lowered_args: &[String],
    caps_absent_from_args: bool,
) -> Option<String> {
    let ctor_method_name = format!("{}_constructor", class.name);
    let ctor_name = ctx
        .methods
        .get(&(class.name.clone(), ctor_method_name))
        .cloned()?;
    // The standalone `<class>_constructor` symbol's signature is the class's
    // OWN ctor params, OR — when the class has no own ctor — the closest
    // ancestor-with-a-ctor's params (codegen/artifacts.rs synthesizes the
    // default ctor `constructor(...args) { super(...args) }` with that adopted
    // signature). Mirror that here so we pass the constructor arguments through
    // this nested-construction path. Reading `param_count` from `class.constructor`
    // alone yielded 0 for a no-own-ctor subclass, so `new Sub(arg)` issued inside a
    // method of `Sub` (the recursion-guarded symbol-call path) dropped every arg —
    // the synthesized ctor's forwarded params then read uninitialized and the
    // inherited `this.x = arg` stored garbage. Pervasive in zod (`new ZodNumber({…})`
    // from `_addCheck`, where ZodNumber has no own ctor and ZodType does).
    let param_count = effective_constructor_param_count(ctx, class);
    let undef_lit = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
    // When the ctor's signature is statically known, build per-param values
    // with the SAME packing rules the inline path uses — a rest param or the
    // synthesized `arguments` param receives a PACKED ARRAY, not a raw
    // positional value. Pre-fix, `new Kid({...})` from a method of Kid (the
    // recursion-guarded symbol-call path) shoved the user arg RAW into the
    // ctor's synthetic `arguments` slot; `super(...arguments)` then spread
    // an object with no `length` and the parent ctor saw zero args
    // (vendored zod's `z.number().int()` chain — `_addCheck` →
    // `new ZodNumber({…})` → `constructor(){ super(...arguments) }`).
    let effective_params: Option<Vec<perry_hir::Param>> = {
        let mut found = class.constructor.as_ref().map(|c| c.params.clone());
        if found.is_none() {
            let mut parent = class.extends_name.as_deref().map(|s| s.to_string());
            while let Some(pname) = parent {
                match ctx.classes.get(&pname).copied() {
                    Some(pc) => {
                        if let Some(pctor) = pc.constructor.as_ref() {
                            found = Some(pctor.params.clone());
                            break;
                        }
                        parent = pc.extends_name.as_deref().map(|s| s.to_string());
                    }
                    None => break,
                }
            }
        }
        found
    };
    let capture_fill = ctx
        .class_ids
        .get(&class.name)
        .copied()
        .map(|cid| CaptureFill {
            cid,
            caps_absent_from_args,
        });
    let mut ctor_values = if let Some(params) = effective_params {
        inline_constructor_param_values_with_class(ctx, &params, lowered_args, capture_fill)
    } else {
        lowered_args.to_vec()
    };
    ctor_values.truncate(param_count);
    while ctor_values.len() < param_count {
        ctor_values.push(undef_lit.clone());
    }

    let mut ctor_args: Vec<(crate::types::LlvmType, &str)> =
        Vec::with_capacity(1 + ctor_values.len());
    ctor_args.push((DOUBLE, obj_box));
    for arg in &ctor_values {
        ctor_args.push((DOUBLE, arg.as_str()));
    }
    Some(ctx.block().call(DOUBLE, &ctor_name, &ctor_args))
}
