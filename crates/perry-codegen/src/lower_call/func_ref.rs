//! User function call via `Expr::FuncRef(fid)` — direct LLVM call to a
//! known per-function symbol, with clamp-pattern intrinsification and
//! rest-parameter bundling.

use anyhow::Result;
use perry_hir::Expr;

use crate::expr::{i32_bool_to_nanbox, i32_to_nanbox, lower_expr, FnCtx};
use crate::nanbox::double_literal;
use crate::native_value::LoweredValue;
use crate::types::{DOUBLE, I1, I32, I64, PTR};

fn typed_i1_signature_note(reps: &[crate::codegen::TypedParamRep]) -> String {
    let first = reps.first().map(|rep| rep.label()).unwrap_or("void");
    if reps.len() <= 1 {
        format!("typed_signature=i1({first})->i1")
    } else {
        format!("typed_signature=i1({first}, ...)->i1")
    }
}

/// Representation-selection Phase 2, Tier A: direct dispatch to a specialized
/// entry. Raw slots are proven by construction at this exact call site. A
/// boxed container proof may add one descriptor diamond for the whole call;
/// a mismatch keeps the permanent boxed fallback.
///
/// - `I32` — integer literal in i32 range.
/// - `F64` — numeric literal (bit-identical raw double).
/// - `TaPtr` — `LocalGet` of a pre-pass-proven, never-reassigned,
///   non-closure-referenced typed-array binding whose kind/const-len match
///   the slot, whose top-level `Stmt::Let` has lowered in THIS body
///   (`spec_ta_ready` — the dominance mirror), and which is not box-backed.
///   All `TaPtr` args must be DISTINCT locals: distinct proven bindings are
///   distinct fresh allocations, which is what justifies the entry's
///   pairwise-noalias view slots.
fn try_emit_spec_static_call(
    ctx: &mut FnCtx<'_>,
    fname: &str,
    plan: &crate::codegen::SpecFnPlan,
    args: &[Expr],
    lowered: &[String],
) -> Option<String> {
    use crate::collectors::SpecParamRep;
    if plan.reps.len() != args.len() || plan.reps.len() != lowered.len() {
        return None;
    }
    // Per-slot proof + raw-arg plan (no IR emitted until every slot matches).
    enum RawArg {
        Double(usize),
        I32Const(i64),
        I32Value(usize),
        TaPtr(usize),
    }
    let mut raw_plan: Vec<RawArg> = Vec::with_capacity(args.len());
    let mut ta_locals: Vec<u32> = Vec::new();
    // Slots whose value is a proven exact integer but whose static window
    // leaves the 32-bit range: they take the fast entry behind one range test.
    let mut range_checked: Vec<usize> = Vec::new();
    for (i, (rep, arg)) in plan.reps.iter().zip(args.iter()).enumerate() {
        match rep {
            SpecParamRep::Boxed => raw_plan.push(RawArg::Double(i)),
            // Plan construction normalizes this call-site-only marker to a
            // boxed slot plus a descriptor before lowering.
            SpecParamRep::NumberArray => return None,
            SpecParamRep::F64 => match arg {
                Expr::Number(_) | Expr::Integer(_) => raw_plan.push(RawArg::Double(i)),
                _ => return None,
            },
            SpecParamRep::I32 => match arg {
                Expr::Integer(n) if i32::try_from(*n).is_ok() => {
                    raw_plan.push(RawArg::I32Const(*n))
                }
                Expr::LocalGet(id) if ctx.integer_locals.contains(id) => {
                    raw_plan.push(RawArg::I32Value(i))
                }
                // #8167: a value DERIVED from this entry's own raw-i32
                // parameters. Without this, a self-recursive call inside a
                // specialized clone always re-enters the generic public
                // symbol, so the clone runs once per top-level call and every
                // recursive step pays dynamic dispatch.
                _ => match spec_i32_derived_window(ctx, arg, 0)? {
                    (lo, hi) if lo >= I32_MIN_I64 && hi <= I32_MAX_I64 => {
                        raw_plan.push(RawArg::I32Value(i))
                    }
                    // No overlap with the slot at all: a range test could only
                    // ever fail, so keep the boxed path and emit no diamond.
                    (lo, hi) if hi < I32_MIN_I64 || lo > I32_MAX_I64 => return None,
                    _ => {
                        range_checked.push(i);
                        raw_plan.push(RawArg::I32Value(i));
                    }
                },
            },
            SpecParamRep::TaPtr { kind, const_len } => {
                let Expr::LocalGet(id) = arg else {
                    return None;
                };
                let Some(binding) = ctx.spec_ta_bindings.get(id) else {
                    return None;
                };
                if binding.kind != *kind
                    || binding.const_len != *const_len
                    || !ctx.spec_ta_ready.contains(id)
                    || ctx.boxed_vars.contains(id)
                    || ctx.prealloc_boxes.contains(id)
                    || ctx.tdz_boxes.contains(id)
                    || ta_locals.contains(id)
                {
                    return None;
                }
                ta_locals.push(*id);
                raw_plan.push(RawArg::TaPtr(i));
            }
        }
    }

    let spec_name = crate::codegen::spec_function_name(fname, &plan.reps);
    let tuple_note: Vec<String> = plan.reps.iter().map(|r| r.label()).collect();

    // Emit the raw argument vector and the specialized call. Factored out so
    // the guard-free and range-checked shapes below cannot drift apart.
    fn emit_raw_args(
        ctx: &mut FnCtx<'_>,
        raw_plan: &[RawArg],
        lowered: &[String],
    ) -> Vec<(crate::types::LlvmType, String)> {
        let mut raw_args_storage: Vec<(crate::types::LlvmType, String)> =
            Vec::with_capacity(raw_plan.len());
        for entry in raw_plan {
            match entry {
                RawArg::Double(i) => raw_args_storage.push((DOUBLE, lowered[*i].clone())),
                RawArg::I32Const(n) => raw_args_storage.push((I32, n.to_string())),
                RawArg::I32Value(i) => {
                    let raw = ctx.block().fptosi(DOUBLE, &lowered[*i], I32);
                    raw_args_storage.push((I32, raw));
                }
                RawArg::TaPtr(i) => {
                    let blk = ctx.block();
                    let bits = blk.bitcast_double_to_i64(&lowered[*i]);
                    let raw = blk.and(I64, &bits, crate::nanbox::POINTER_MASK_I64);
                    raw_args_storage.push((I64, raw));
                }
            }
        }
        raw_args_storage
    }

    let check_descriptors = matches!(plan.dispatch, crate::codegen::SpecDispatch::Static);
    if !range_checked.is_empty() || (check_descriptors && plan.guards.iter().any(Option::is_some)) {
        // One diamond for the whole call: every range-checked slot's test is
        // ANDed, so the fast arm is entered only when EVERY raw slot's
        // contract holds. The fallback is the permanent boxed ABI, which is
        // what this site would have emitted without the specialization.
        let mut guard: Option<String> = None;
        for i in &range_checked {
            let value = lowered[*i].clone();
            let blk = ctx.block();
            let ge = blk.fcmp("oge", &value, &double_literal(f64::from(i32::MIN)));
            let le = blk.fcmp("ole", &value, &double_literal(f64::from(i32::MAX)));
            let ok = blk.and(I1, &ge, &le);
            guard = Some(match guard {
                Some(prev) => ctx.block().and(I1, &prev, &ok),
                None => ok,
            });
        }
        if check_descriptors {
            for (i, descriptor) in plan.guards.iter().enumerate() {
                let Some(descriptor) = descriptor else {
                    continue;
                };
                let ok = if let Some(rep) =
                    crate::codegen::scalar_descriptor_rep(&descriptor.descriptor)
                {
                    crate::codegen::emit_typed_arg_guard(ctx.block(), rep, &lowered[i])
                } else {
                    let raw = ctx.block().call(
                        I32,
                        "js_param_type_guard",
                        &[
                            (DOUBLE, lowered[i].as_str()),
                            (PTR, &format!("@{}", descriptor.descriptor_name)),
                            (I32, &descriptor.descriptor.len().to_string()),
                        ],
                    );
                    ctx.block().icmp_ne(I32, &raw, "0")
                };
                guard = Some(match guard {
                    Some(prev) => ctx.block().and(I1, &prev, &ok),
                    None => ok,
                });
            }
        }
        let guard = guard?;
        let fast_idx = ctx.new_block("spec_checked_call.fast");
        let fallback_idx = ctx.new_block("spec_checked_call.fallback");
        let merge_idx = ctx.new_block("spec_checked_call.merge");
        let fast_label = ctx.block_label(fast_idx);
        let fallback_label = ctx.block_label(fallback_idx);
        let merge_label = ctx.block_label(merge_idx);
        ctx.block().cond_br(&guard, &fast_label, &fallback_label);

        ctx.current_block = fast_idx;
        let raw_args_storage = emit_raw_args(ctx, &raw_plan, lowered);
        let call_args: Vec<(crate::types::LlvmType, &str)> = raw_args_storage
            .iter()
            .map(|(ty, v)| (*ty, v.as_str()))
            .collect();
        let fast_value = ctx.block().call(DOUBLE, &spec_name, &call_args);
        let after_fast = ctx.block().label.clone();
        if !ctx.block().is_terminated() {
            ctx.block().br(&merge_label);
        }

        ctx.current_block = fallback_idx;
        let boxed_args: Vec<(crate::types::LlvmType, &str)> =
            lowered.iter().map(|v| (DOUBLE, v.as_str())).collect();
        let fallback_value = ctx.block().call(DOUBLE, fname, &boxed_args);
        let after_fallback = ctx.block().label.clone();
        if !ctx.block().is_terminated() {
            ctx.block().br(&merge_label);
        }

        ctx.current_block = merge_idx;
        let result = ctx.block().phi(
            DOUBLE,
            &[
                (fast_value.as_str(), after_fast.as_str()),
                (fallback_value.as_str(), after_fallback.as_str()),
            ],
        );
        ctx.record_lowered_value(
            "Call",
            None,
            "spec_abi_checked_call",
            &LoweredValue::js_value(result.clone()),
            None,
            None,
            None,
            false,
            false,
            vec![
                format!("spec_call=checked; symbol={spec_name}; boxed_fallback={fname}"),
                format!("tuple={}", tuple_note.join(",")),
                format!("range_checked_slots={range_checked:?}"),
                format!(
                    "descriptor_checked_slots={:?}",
                    plan.guards
                        .iter()
                        .enumerate()
                        .filter_map(|(i, guard)| {
                            (check_descriptors && guard.is_some()).then_some(i)
                        })
                        .collect::<Vec<_>>()
                ),
            ],
        );
        return Some(result);
    }

    let raw_args_storage = emit_raw_args(ctx, &raw_plan, lowered);
    let call_args: Vec<(crate::types::LlvmType, &str)> = raw_args_storage
        .iter()
        .map(|(ty, v)| (*ty, v.as_str()))
        .collect();
    let result = ctx.block().call(DOUBLE, &spec_name, &call_args);
    ctx.record_lowered_value(
        "Call",
        None,
        "spec_abi_static_call",
        &LoweredValue::js_value(result.clone()),
        None,
        None,
        None,
        false,
        false,
        vec![
            format!("spec_call=static; symbol={spec_name}"),
            format!("tuple={}", tuple_note.join(",")),
        ],
    );
    Some(result)
}

const I32_MIN_I64: i64 = i32::MIN as i64;
const I32_MAX_I64: i64 = i32::MAX as i64;

/// Beyond this magnitude a `double` can no longer hold every integer, so the
/// JS `Number` the caller actually computes stops equalling the integer this
/// proof reasons about.
const SPEC_I32_EXACT_INTEGER_LIMIT: i64 = (1i64 << 53) - 1;

/// Static value window of a call argument built ONLY from integer literals
/// and parameters that the ENCLOSING specialized entry binds as a raw LLVM
/// `i32`, composed with `+` and `-`.
///
/// The obligation is the raw-`I32` slot's contract, which
/// `js_typed_i32_arg_guard` (`perry-runtime/src/native_abi.rs`) states
/// exactly: finite, INTEGRAL, inside the signed 32-bit range, and **not
/// `-0`**. This shape discharges three of the four statically:
///
/// * *finite and integral* — an `i32` parameter is an exact integer, an
///   `Expr::Integer` literal is an exact integer, and `+`/`-` over exact
///   integers whose magnitudes stay under 2^53 is exact in `double`, which
///   the window cap below enforces at every node.
/// * *not `-0`* — `sitofp` of an `i32` is never `-0` and an integer literal
///   is never `-0`, and IEEE-754 round-to-nearest yields `-0` from `x + y`
///   only when both operands are `-0`, and from `x - y` only when `x` is
///   `-0` and `y` is `+0`. No leaf is `-0`, so by induction no node is.
///
/// The fourth — the 32-bit range — is what the returned window ANSWERS, and
/// it is precisely where "it is an integer, ship it" would be wrong:
/// `n - 1` for `n: i32` is `[-2^31 - 1, 2^31 - 2]`, one value wider than the
/// slot. The caller either proves containment or emits a range test.
///
/// Multiplication is deliberately absent. `n * 0` with `n < 0` is `-0`, which
/// the slot contract rejects, and a product of two `i32`s leaves the
/// exact-integer window — both would need their own arguments.
fn spec_i32_derived_window(ctx: &FnCtx<'_>, expr: &Expr, depth: usize) -> Option<(i64, i64)> {
    if depth > 32 {
        return None;
    }
    let (lo, hi) = match expr {
        Expr::Integer(n) => (*n, *n),
        Expr::LocalGet(id) if ctx.spec_i32_params.contains(id) => (I32_MIN_I64, I32_MAX_I64),
        Expr::Binary { op, left, right } => {
            let (llo, lhi) = spec_i32_derived_window(ctx, left, depth + 1)?;
            let (rlo, rhi) = spec_i32_derived_window(ctx, right, depth + 1)?;
            match op {
                perry_hir::BinaryOp::Add => (llo.checked_add(rlo)?, lhi.checked_add(rhi)?),
                perry_hir::BinaryOp::Sub => (llo.checked_sub(rhi)?, lhi.checked_sub(rlo)?),
                _ => return None,
            }
        }
        _ => return None,
    };
    (lo >= -SPEC_I32_EXACT_INTEGER_LIMIT && hi <= SPEC_I32_EXACT_INTEGER_LIMIT).then_some((lo, hi))
}

/// Phase 2, Tier B: only a call site whose current facts prove every
/// declaration-guarded slot may bypass the public wrapper. Unknown and
/// indirect callers target that wrapper, which owns the runtime guard and
/// generic fallback.
fn try_emit_spec_guarded_call(
    ctx: &mut FnCtx<'_>,
    fname: &str,
    plan: &crate::codegen::SpecFnPlan,
    args: &[Expr],
    lowered: &[String],
) -> Option<String> {
    if plan.reps.len() != args.len()
        || plan.reps.len() != lowered.len()
        || !plan.guards.iter().zip(args.iter()).all(|(guard, arg)| {
            guard
                .as_ref()
                .is_none_or(|candidate| guarded_argument_proves(ctx, arg, &candidate.proof))
        })
    {
        return None;
    }
    try_emit_spec_static_call(ctx, fname, plan, args, lowered)
}
fn normalize_guard_type(ctx: &FnCtx<'_>, ty: &perry_hir::types::Type) -> perry_hir::types::Type {
    let mut current = ty.clone();
    for _ in 0..16 {
        let perry_hir::types::Type::Named(name) = &current else {
            break;
        };
        let Some(next) = ctx.type_aliases.get(name) else {
            break;
        };
        current = next.clone();
    }
    current
}

fn guarded_type_assignable(
    ctx: &FnCtx<'_>,
    actual: &perry_hir::types::Type,
    expected: &perry_hir::types::Type,
    depth: usize,
) -> bool {
    use perry_hir::types::Type;
    if actual == expected {
        return true;
    }
    if depth > 32 {
        return false;
    }
    let actual = normalize_guard_type(ctx, actual);
    let expected = normalize_guard_type(ctx, expected);
    if actual == expected {
        return true;
    }
    match (&actual, &expected) {
        (Type::Never, _) => true,
        (Type::Int32, Type::Number) | (Type::StringLiteral(_), Type::String) => true,
        (Type::Union(actual), _) => actual
            .iter()
            .all(|variant| guarded_type_assignable(ctx, variant, &expected, depth + 1)),
        (_, Type::Union(expected)) => expected
            .iter()
            .any(|variant| guarded_type_assignable(ctx, &actual, variant, depth + 1)),
        (Type::Array(actual), Type::Array(expected)) => {
            guarded_type_assignable(ctx, actual, expected, depth + 1)
        }
        (Type::Tuple(actual), Type::Tuple(expected)) if actual.len() == expected.len() => actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| guarded_type_assignable(ctx, actual, expected, depth + 1)),
        (Type::Object(actual), Type::Object(expected)) => {
            expected.properties.iter().all(|(name, expected_field)| {
                !expected_field.optional
                    && actual.properties.get(name).is_some_and(|actual_field| {
                        !actual_field.optional
                            && guarded_type_assignable(
                                ctx,
                                &actual_field.ty,
                                &expected_field.ty,
                                depth + 1,
                            )
                    })
            })
        }
        _ => false,
    }
}

fn guarded_property_type(
    ctx: &FnCtx<'_>,
    owner: &perry_hir::types::Type,
    property: &str,
    depth: usize,
) -> Option<perry_hir::types::Type> {
    use perry_hir::types::Type;
    if depth > 16 {
        return None;
    }
    match owner {
        Type::Named(name) => {
            if let Some(alias) = ctx.type_aliases.get(name) {
                return guarded_property_type(ctx, alias, property, depth + 1);
            }
            if let Some(interface) = ctx.interfaces.get(name) {
                return interface
                    .properties
                    .iter()
                    .find(|candidate| candidate.name == property)
                    .map(|candidate| candidate.ty.clone());
            }
            let class = ctx.classes.get(name)?;
            if let Some(field) = class.fields.iter().find(|field| field.name == property) {
                return Some(field.ty.clone());
            }
            let mut parent = class.extends_name.as_deref();
            while let Some(name) = parent {
                let class = ctx.classes.get(name)?;
                if let Some(field) = class.fields.iter().find(|field| field.name == property) {
                    return Some(field.ty.clone());
                }
                parent = class.extends_name.as_deref();
            }
            None
        }
        Type::Object(object) => object
            .properties
            .get(property)
            .map(|candidate| candidate.ty.clone()),
        Type::Union(variants) => {
            // A path is unconditional evidence only when every possible arm
            // declares the field with the same type. Branch-specific
            // narrowing is not represented in FnCtx; skipping an arm that
            // lacks the field would turn that arm's runtime `undefined` into
            // a false proof.
            let mut found: Option<Type> = None;
            for variant in variants {
                let candidate = guarded_property_type(ctx, variant, property, depth + 1)?;
                if found.as_ref().is_some_and(|existing| {
                    normalize_guard_type(ctx, existing) != normalize_guard_type(ctx, &candidate)
                }) {
                    return None;
                }
                found = Some(candidate);
            }
            found
        }
        _ => None,
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum GuardedLiteralRelation {
    Equal,
    NotEqual,
    Unknown,
}

fn guarded_string_literal_relation(
    ctx: &FnCtx<'_>,
    ty: &perry_hir::types::Type,
    literal: &str,
    depth: usize,
) -> GuardedLiteralRelation {
    use perry_hir::types::Type;
    if depth > 16 {
        return GuardedLiteralRelation::Unknown;
    }
    match normalize_guard_type(ctx, ty) {
        Type::StringLiteral(value) if value == literal => GuardedLiteralRelation::Equal,
        Type::StringLiteral(_) => GuardedLiteralRelation::NotEqual,
        Type::Union(variants) => {
            let mut relation = None;
            for variant in variants {
                let candidate = guarded_string_literal_relation(ctx, &variant, literal, depth + 1);
                if candidate == GuardedLiteralRelation::Unknown
                    || relation.is_some_and(|existing| existing != candidate)
                {
                    return GuardedLiteralRelation::Unknown;
                }
                relation = Some(candidate);
            }
            relation.unwrap_or(GuardedLiteralRelation::Unknown)
        }
        _ => GuardedLiteralRelation::Unknown,
    }
}

fn guarded_union_subset(
    ctx: &FnCtx<'_>,
    proof: &perry_hir::types::Type,
    property: &str,
    literal: &str,
    keep_equal: bool,
) -> Option<perry_hir::types::Type> {
    use perry_hir::types::Type;
    let Type::Union(variants) = normalize_guard_type(ctx, proof) else {
        return None;
    };
    let original_len = variants.len();
    let mut retained = Vec::new();
    for variant in variants {
        let relation = guarded_property_type(ctx, &variant, property, 0)
            .map(|field| guarded_string_literal_relation(ctx, &field, literal, 0))
            .unwrap_or(GuardedLiteralRelation::Unknown);
        let retain = match relation {
            GuardedLiteralRelation::Equal => keep_equal,
            GuardedLiteralRelation::NotEqual => !keep_equal,
            // A broad string field, an absent field, or an unresolved type
            // can satisfy either branch at runtime. It may not be discarded.
            GuardedLiteralRelation::Unknown => true,
        };
        if retain {
            retained.push(variant);
        }
    }
    if retained.is_empty() || retained.len() == original_len {
        return None;
    }
    if retained.len() == 1 {
        retained.pop()
    } else {
        Some(Type::Union(retained))
    }
}

/// Narrow an entry-guarded discriminated union for the two successors of a
/// strict string comparison. The returned facts are branch-local: callers
/// must restore the original proof after lowering each successor.
///
/// This deliberately starts from `stable_local_type_proof`, never from a
/// declaration. Consequently `if (value.kind === "x")` cannot turn an erased
/// annotation into evidence; it can only refine a value already accepted by
/// the public ordinary-parameter guard (or otherwise constructively proven).
pub(crate) fn guarded_discriminant_branch_proofs(
    ctx: &FnCtx<'_>,
    condition: &Expr,
) -> Option<(
    u32,
    Option<perry_hir::types::Type>,
    Option<perry_hir::types::Type>,
)> {
    use perry_hir::CompareOp;

    let Expr::Compare { op, left, right } = condition else {
        return None;
    };
    if !matches!(op, CompareOp::Eq | CompareOp::Ne) {
        return None;
    }
    fn discriminant_path(ctx: &FnCtx<'_>, expr: &Expr) -> Option<(u32, String)> {
        match expr {
            Expr::PropertyGet {
                object, property, ..
            } => {
                let Expr::LocalGet(owner_id) = object.as_ref() else {
                    return None;
                };
                Some((*owner_id, property.clone()))
            }
            Expr::LocalGet(alias_id) if !ctx.reassigned_locals.contains(alias_id) => {
                ctx.guarded_discriminant_aliases.get(alias_id).cloned()
            }
            _ => None,
        }
    }

    let (id, property, literal) = match (left.as_ref(), right.as_ref()) {
        (candidate, Expr::String(literal)) => {
            let (id, property) = discriminant_path(ctx, candidate)?;
            (id, property, literal)
        }
        (Expr::String(literal), candidate) => {
            let (id, property) = discriminant_path(ctx, candidate)?;
            (id, property, literal)
        }
        _ => return None,
    };
    let proof = ctx.stable_local_type_proof(&id)?;
    let equal = guarded_union_subset(ctx, proof, &property, literal, true);
    let not_equal = guarded_union_subset(ctx, proof, &property, literal, false);
    let (then_proof, else_proof) = if matches!(op, CompareOp::Eq) {
        (equal, not_equal)
    } else {
        (not_equal, equal)
    };
    (then_proof.is_some() || else_proof.is_some()).then_some((id, then_proof, else_proof))
}

pub(crate) fn guarded_path_type(ctx: &FnCtx<'_>, expr: &Expr) -> Option<perry_hir::types::Type> {
    use perry_hir::types::{ObjectType, PropertyInfo, Type};
    match expr {
        Expr::LocalGet(id) => ctx.stable_local_type_proof(id).cloned(),
        Expr::PropertyGet {
            object, property, ..
        } => {
            let owner = guarded_path_type(ctx, object)?;
            guarded_property_type(ctx, &owner, property, 0)
        }
        Expr::IndexGet { object, index } => {
            let owner = normalize_guard_type(ctx, &guarded_path_type(ctx, object)?);
            match owner {
                Type::Array(element) => Some(*element),
                Type::Tuple(elements) if !elements.is_empty() => match index.as_ref() {
                    Expr::Integer(index) => elements.get(usize::try_from(*index).ok()?).cloned(),
                    _ if elements.windows(2).all(|pair| pair[0] == pair[1]) => {
                        elements.first().cloned()
                    }
                    _ => None,
                },
                Type::Generic { base, type_args } if base == "Array" && type_args.len() == 1 => {
                    type_args.into_iter().next()
                }
                _ => None,
            }
        }
        Expr::Array(elements) => {
            if elements.is_empty() {
                return Some(Type::Array(Box::new(Type::Never)));
            }
            let mut element_types = Vec::new();
            for element in elements {
                let ty = guarded_path_type(ctx, element)?;
                if !element_types.contains(&ty) {
                    element_types.push(ty);
                }
            }
            let element = if element_types.len() == 1 {
                element_types.pop().unwrap()
            } else {
                Type::Union(element_types)
            };
            Some(Type::Array(Box::new(element)))
        }
        Expr::New {
            class_name, args, ..
        } if class_name.starts_with("__AnonShape_") => {
            let class = ctx.classes.get(class_name)?;
            if class.fields.len() != args.len() {
                return None;
            }
            let mut properties = std::collections::HashMap::new();
            let mut order = Vec::new();
            for (field, arg) in class.fields.iter().zip(args) {
                order.push(field.name.clone());
                properties.insert(
                    field.name.clone(),
                    PropertyInfo {
                        ty: guarded_path_type(ctx, arg)?,
                        optional: false,
                        readonly: false,
                    },
                );
            }
            Some(Type::Object(ObjectType {
                name: None,
                properties,
                property_order: Some(order),
                index_signature: None,
            }))
        }
        Expr::Conditional {
            then_expr,
            else_expr,
            ..
        } => {
            let then_ty = guarded_path_type(ctx, then_expr)?;
            let else_ty = guarded_path_type(ctx, else_expr)?;
            if guarded_type_assignable(ctx, &then_ty, &else_ty, 0) {
                Some(else_ty)
            } else if guarded_type_assignable(ctx, &else_ty, &then_ty, 0) {
                Some(then_ty)
            } else {
                Some(Type::Union(vec![then_ty, else_ty]))
            }
        }
        Expr::String(value) => Some(Type::StringLiteral(value.clone())),
        Expr::WtfString(_) => Some(Type::String),
        Expr::Bool(_) => Some(Type::Boolean),
        Expr::Number(_) => Some(Type::Number),
        Expr::Integer(value) if i32::try_from(*value).is_ok() => Some(Type::Int32),
        Expr::Integer(_) => Some(Type::Number),
        Expr::Null => Some(Type::Null),
        Expr::Undefined | Expr::Void(_) => Some(Type::Void),
        Expr::Call { .. } => guarded_call_return_proof(ctx, expr),
        // #8169: a Tier-B clone's entry guard gives its boxed Number
        // parameters real runtime proofs, and arithmetic derived from those
        // parameters constructs another Number. Let a recursive `f(n - 1)`
        // therefore re-enter `$spec_b` instead of paying the public guard on
        // every edge.
        //
        // Use the canonical-value predicate rather than `is_numeric_expr`
        // alone. The latter deliberately admits some dynamic/BigInt-capable
        // arithmetic for lowering decisions; this proof is used to BYPASS a
        // runtime type guard, so it must exclude values that can still be a
        // boxed BigInt or arise only from an unenforced annotation.
        _ if crate::type_analysis::expr_produces_canonical_raw_f64(ctx, expr) => Some(Type::Number),
        _ => None,
    }
}

fn guarded_argument_proves(
    ctx: &FnCtx<'_>,
    expr: &Expr,
    expected: &perry_hir::types::Type,
) -> bool {
    let actual = guarded_path_type(ctx, expr);
    let Some(actual) = actual else {
        return false;
    };
    let actual = normalize_guard_type(ctx, &actual);
    let expected = normalize_guard_type(ctx, expected);
    guarded_type_assignable(ctx, &actual, &expected, 0)
}

/// A proof established by the expression's runtime construction or by a
/// constructively verified guarded call. Used only to seed clone-local facts;
/// the generic body never consults declaration metadata through this route.
pub(crate) fn guarded_expr_proof(
    ctx: &FnCtx<'_>,
    expr: &Expr,
    expected: &perry_hir::types::Type,
) -> Option<perry_hir::types::Type> {
    guarded_argument_proves(ctx, expr, expected).then(|| expected.clone())
}

/// Return evidence from a specialized call is usable only when the producer's
/// body was constructively verified and this exact call's live arguments prove
/// every descriptor slot. A generic fallback result never reaches this path.
pub(crate) fn guarded_call_return_proof(
    ctx: &FnCtx<'_>,
    expr: &Expr,
) -> Option<perry_hir::types::Type> {
    let Expr::Call { callee, args, .. } = expr else {
        return None;
    };
    let Expr::FuncRef(function_id) = callee.as_ref() else {
        return None;
    };
    let plan = ctx.spec_abi_functions.get(function_id)?;
    let proof = ctx.spec_return_proofs.get(function_id)?;
    if plan.reps.len() != args.len()
        || plan.guards.len() != args.len()
        || !plan.guards.iter().zip(args).all(|(guard, arg)| {
            guard
                .as_ref()
                .is_some_and(|candidate| guarded_argument_proves(ctx, arg, &candidate.proof))
        })
    {
        return None;
    }
    Some(proof.clone())
}

fn typed_signature_note(
    ret: &str,
    reps: &[crate::codegen::TypedParamRep],
    closure_arg: bool,
) -> String {
    let first = reps.first().map(|rep| rep.label()).unwrap_or("void");
    let first = if closure_arg { "i64 closure" } else { first };
    if reps.is_empty() {
        format!("typed_signature={ret}({first})->{ret}")
    } else if reps.len() == 1 && !closure_arg {
        format!("typed_signature={ret}({first})->{ret}")
    } else {
        format!("typed_signature={ret}({first}, ...)->{ret}")
    }
}

pub fn try_lower_func_ref_call(
    ctx: &mut FnCtx<'_>,
    callee: &Expr,
    args: &[Expr],
) -> Result<Option<String>> {
    // User function call via FuncRef.
    let Expr::FuncRef(fid) = callee else {
        return Ok(None);
    };
    // (Issue #436 plan #1) Clamp-pattern fast path: when the callee
    // is a function recognized as `clampIdx(v, lo, hi)` or
    // `clampU8(v)` and we're being lowered in an f64-required
    // context, emit `@llvm.smin.i32` / `@llvm.smax.i32` directly +
    // `sitofp` to double, mirroring the i32 path in
    // `lower_expr_as_i32`. The HIR inliner is configured to leave
    // these calls intact (`is_clamp3`/`is_clamp_u8` short-circuit
    // `is_inlinable`) so this path fires at every call site and the
    // `dowhile/break` shape that blocked LLVM's auto-vectorizer
    // never appears in the IR.
    //
    // clamp3-shaped functions return one of their ARGUMENTS verbatim, so
    // the i32 intrinsification is only sound when every argument is
    // provably i32-lowerable (`can_lower_expr_as_i32` — whose contract
    // `lower_expr_as_i32` requires anyway). Unconditional intrinsification
    // fptosi'd fractional doubles (`clamp3(2.5, 0, 5)` returned 2) and
    // NaN-boxed pointers (i32::MIN — the #4785 `(number).method is not a
    // function` bug class) at every call site. Non-i32 arguments fall
    // through to the ordinary direct call, whose compiled body has the
    // correct verbatim-return semantics. clampU8 stays unconditional: its
    // detector verifies the body ends in `return v | 0`, and fptosi +
    // smax(0)/smin(255) agrees with that coercion for every f64 input
    // (out-of-range values hit the clamp bounds first; NaN and boxed
    // pointers coerce to 0 either way).
    if ctx.clamp3_functions.contains(fid) && args.len() == 3 {
        let args_are_i32 = args.iter().all(|a| {
            crate::expr::can_lower_expr_as_i32(
                a,
                &ctx.i32_counter_slots,
                ctx.flat_const_arrays,
                &ctx.array_row_aliases,
                ctx.integer_locals,
                &ctx.const_number_locals,
                ctx.clamp3_functions,
                ctx.clamp_u8_functions,
                ctx.integer_returning_functions,
                ctx.i32_identity_functions,
            )
        });
        if args_are_i32 {
            let v = crate::expr::lower_expr_as_i32(ctx, &args[0])?;
            let lo = crate::expr::lower_expr_as_i32(ctx, &args[1])?;
            let hi = crate::expr::lower_expr_as_i32(ctx, &args[2])?;
            let blk = ctx.block();
            let r1 = blk.fresh_reg();
            blk.emit_raw(format!(
                "{} = call i32 @llvm.smax.i32(i32 {}, i32 {})",
                r1, v, lo
            ));
            let r2 = blk.fresh_reg();
            blk.emit_raw(format!(
                "{} = call i32 @llvm.smin.i32(i32 {}, i32 {})",
                r2, r1, hi
            ));
            return Ok(Some(blk.sitofp(I32, &r2, DOUBLE)));
        }
    }
    if ctx.clamp_u8_functions.contains(fid) && args.len() == 1 {
        let v = crate::expr::lower_expr_as_i32(ctx, &args[0])?;
        let blk = ctx.block();
        let r1 = blk.fresh_reg();
        blk.emit_raw(format!(
            "{} = call i32 @llvm.smax.i32(i32 {}, i32 0)",
            r1, v
        ));
        let r2 = blk.fresh_reg();
        blk.emit_raw(format!(
            "{} = call i32 @llvm.smin.i32(i32 {}, i32 255)",
            r2, r1
        ));
        return Ok(Some(blk.sitofp(I32, &r2, DOUBLE)));
    }

    let Some(fname) = ctx.func_names.get(fid).cloned() else {
        for a in args {
            let _ = lower_expr(ctx, a)?;
        }
        return Ok(Some(double_literal(0.0)));
    };

    // Rest parameter handling: if the called function has a
    // rest parameter, bundle all trailing args (those at and
    // beyond the rest position) into an array literal and
    // pass that as a single argument.
    let sig = ctx.func_signatures.get(fid).copied();
    let (declared_count, has_rest, _, synthetic_is_rest) =
        sig.unwrap_or((args.len(), false, false, false));
    // #7154: the same-module twin of `extern_func.rs`'s cross-module path.
    //
    // #7240 fixed the cross-module lowering and needed a two-file fixture to do
    // it, precisely because a same-file callee resolves here instead — so the
    // identical defect sat one `else` away, unreached by that PR's test. All
    // four arms below lowered their arguments into bare SSA registers and then
    // held them across work that allocates: the rest arms across
    // `js_array_alloc` + a `js_array_push_f64` per element (and the first arm
    // across TWO such arrays), the plain arm across the later arguments' own
    // lowering.
    //
    // The guard is released after the call rather than here — see the
    // `temp_root_release` below the dispatch chain. That placement is the whole
    // reason this was not folded into #7240: `lowered` is consumed by four
    // specialized-ABI dispatch paths with block-splitting diamonds, so the
    // release has to sit in the merge block that post-dominates all of them,
    // not next to the lowering.
    let mut lowered: Vec<String> = Vec::with_capacity(declared_count);
    let arg_group: crate::rooting::RootedGroup<'_>;
    if ctx.func_synthetic_arguments.contains(fid) && has_rest && !synthetic_is_rest {
        // #1816: a real `...rest` AND a synthetic `arguments`, over the same
        // argument list at two different offsets.
        let fixed_count = declared_count.saturating_sub(2);
        let (values, guard) = super::lower_rest_call_args_rooted(
            ctx,
            args,
            fixed_count,
            &[
                super::RestBundle {
                    from: fixed_count,
                    mark_arguments_object: false,
                },
                super::RestBundle {
                    from: 0,
                    mark_arguments_object: false,
                },
            ],
        )?;
        arg_group = guard;
        lowered.extend(values);
    } else if has_rest && ctx.func_synthetic_arguments.contains(fid) {
        let fixed_count = declared_count.saturating_sub(1);
        let (values, guard) = super::lower_rest_call_args_rooted(
            ctx,
            args,
            fixed_count,
            &[super::RestBundle {
                from: 0,
                mark_arguments_object: true,
            }],
        )?;
        arg_group = guard;
        lowered.extend(values);
    } else if has_rest {
        // Rest is always the LAST declared param. Pass the
        // first (declared_count - 1) args as-is, then bundle
        // the rest into an array.
        let fixed_count = declared_count.saturating_sub(1);
        let (values, guard) = super::lower_rest_call_args_rooted(
            ctx,
            args,
            fixed_count,
            &[super::RestBundle {
                from: fixed_count,
                mark_arguments_object: false,
            }],
        )?;
        arg_group = guard;
        lowered.extend(values);
    } else {
        let (values, guard) = super::lower_call_args_rooted(ctx, args)?;
        arg_group = guard;
        lowered.extend(values);
    }
    let arg_slices: Vec<(crate::types::LlvmType, &str)> =
        lowered.iter().map(|s| (DOUBLE, s.as_str())).collect();

    // OrdinaryCallBindThis for a receiverless call: `f()` binds `this` to
    // undefined (sloppy bodies then substitute globalThis at the read).
    // Without the reset, a bare call inside a method body leaks the
    // enclosing dispatch's IMPLICIT_THIS into the callee — a nested
    // `function inner(){ return this; }` called as `inner()` inside
    // `o.m()` must NOT see `o` (#3576). Gated on the callee actually
    // reading dynamic `this` so ordinary helper calls pay nothing. Args
    // are lowered BEFORE the reset: `this` inside an argument expression
    // still sees the enclosing binding.
    let resets_this = ctx.funcs_reading_dynamic_this.contains(fid);
    // #7211: rooted save/restore. The value displaced here is the ENCLOSING
    // method's receiver, held across the callee body — arbitrary user code.
    let prev_this = if resets_this {
        let undef = double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
        Some(crate::rooting::implicit_this_save(ctx, &undef))
    } else {
        None
    };
    // Representation-selection Phase 2: specialized-ABI dispatch. Tier A
    // (static, guard-free) fires only when every slot's proof holds AT THIS
    // SITE; Tier B keeps the guarded-diamond shape. Any mismatch falls
    // through to the existing typed/generic chain — the public boxed entry is
    // the permanent fallback ABI.
    let spec_result: Option<String> = if crate::codegen::spec_abi_enabled()
        && !resets_this
        && !has_rest
        && !ctx.func_synthetic_arguments.contains(fid)
        && declared_count == args.len()
    {
        match ctx.spec_abi_functions.get(fid).cloned() {
            Some(plan) => match plan.dispatch {
                crate::codegen::SpecDispatch::Static => {
                    try_emit_spec_static_call(ctx, &fname, &plan, args, &lowered)
                }
                crate::codegen::SpecDispatch::Guarded => {
                    if plan.guards.iter().zip(args.iter()).all(|(guard, arg)| {
                        guard.as_ref().is_none_or(|candidate| {
                            guarded_argument_proves(ctx, arg, &candidate.proof)
                        })
                    }) {
                        try_emit_spec_guarded_call(ctx, &fname, &plan, args, &lowered)
                    } else {
                        None
                    }
                }
            },
            None => None,
        }
    } else {
        None
    };
    // No `spec_result.is_none()` gate on the typed-clone candidate arms: plan
    // selection makes the spec and typed-clone sets mutually exclusive, and
    // the `if let` chain below consumes `spec_result` first anyway.
    let typed_f64_call_param_reps = if !resets_this
        && !has_rest
        && !ctx.func_synthetic_arguments.contains(fid)
        && ctx.typed_f64_functions.contains(fid)
        && declared_count == args.len()
    {
        ctx.typed_i1_function_param_reps
            .get(fid)
            .filter(|reps| crate::codegen::typed_param_reps_match_args(ctx, reps, args))
            .cloned()
    } else {
        None
    };
    let typed_i32_call_param_reps = if !resets_this
        && !has_rest
        && !ctx.func_synthetic_arguments.contains(fid)
        && ctx.typed_i32_functions.contains(fid)
        && declared_count == args.len()
    {
        ctx.typed_i1_function_param_reps
            .get(fid)
            .filter(|reps| crate::codegen::typed_param_reps_match_args(ctx, reps, args))
            .cloned()
    } else {
        None
    };
    let typed_string_call_param_reps = if !resets_this
        && !has_rest
        && !ctx.func_synthetic_arguments.contains(fid)
        && ctx.typed_string_functions.contains(fid)
        && declared_count == args.len()
    {
        ctx.typed_i1_function_param_reps
            .get(fid)
            .filter(|reps| crate::codegen::typed_param_reps_match_args(ctx, reps, args))
            .cloned()
    } else {
        None
    };
    let typed_i1_call_param_reps = if !resets_this
        && !has_rest
        && !ctx.func_synthetic_arguments.contains(fid)
        && ctx.typed_i1_functions.contains(fid)
        && declared_count == args.len()
    {
        ctx.typed_i1_function_param_reps
            .get(fid)
            .filter(|reps| crate::codegen::typed_param_reps_match_args(ctx, reps, args))
            .cloned()
    } else {
        None
    };
    let result = if let Some(spec) = spec_result {
        spec
    } else if let Some(reps) = typed_f64_call_param_reps {
        let typed_name = crate::codegen::typed_f64_function_name(&fname);
        let generic_body_name = crate::codegen::generic_function_body_name(&fname);
        let mut guard: Option<String> = None;
        for (value, rep) in lowered.iter().zip(reps.iter()) {
            let ok = crate::codegen::emit_typed_arg_guard(ctx.block(), *rep, value);
            guard = Some(match guard {
                Some(prev) => ctx.block().and(I1, &prev, &ok),
                None => ok,
            });
        }
        let fast_idx = ctx.new_block("typed_f64_call.fast");
        let fallback_idx = ctx.new_block("typed_f64_call.fallback");
        let merge_idx = ctx.new_block("typed_f64_call.merge");
        let fast_label = ctx.block_label(fast_idx);
        let fallback_label = ctx.block_label(fallback_idx);
        let merge_label = ctx.block_label(merge_idx);
        if let Some(guard) = guard {
            ctx.block().cond_br(&guard, &fast_label, &fallback_label);
        } else {
            ctx.block().br(&fast_label);
        }

        ctx.current_block = fast_idx;
        let mut typed_args_storage: Vec<String> = Vec::with_capacity(lowered.len());
        for (value, rep) in lowered.iter().zip(reps.iter()) {
            typed_args_storage.push(crate::codegen::emit_typed_arg_to_raw(
                ctx.block(),
                *rep,
                value,
            ));
        }
        let typed_args: Vec<(crate::types::LlvmType, &str)> = typed_args_storage
            .iter()
            .zip(reps.iter())
            .map(|(s, rep)| (rep.llvm_ty(), s.as_str()))
            .collect();
        let fast_value = ctx.block().call(DOUBLE, &typed_name, &typed_args);
        let after_fast = ctx.block().label.clone();
        if !ctx.block().is_terminated() {
            ctx.block().br(&merge_label);
        }

        ctx.current_block = fallback_idx;
        let fallback_value = ctx.block().call(DOUBLE, &generic_body_name, &arg_slices);
        let after_fallback = ctx.block().label.clone();
        if !ctx.block().is_terminated() {
            ctx.block().br(&merge_label);
        }

        ctx.current_block = merge_idx;
        let result = ctx.block().phi(
            DOUBLE,
            &[
                (fast_value.as_str(), after_fast.as_str()),
                (fallback_value.as_str(), after_fallback.as_str()),
            ],
        );
        ctx.record_lowered_value(
            "Call",
            None,
            "typed_f64_func_ref_call",
            &LoweredValue::f64(result.clone()),
            None,
            None,
            None,
            false,
            false,
            vec![
                format!("typed_clone={typed_name}; generic_body={generic_body_name}"),
                typed_signature_note("f64", &reps, false),
            ],
        );
        result
    } else if let Some(reps) = typed_i32_call_param_reps {
        let typed_name = crate::codegen::typed_i32_function_name(&fname);
        let generic_body_name = crate::codegen::generic_function_body_name(&fname);
        let mut guard: Option<String> = None;
        for (value, rep) in lowered.iter().zip(reps.iter()) {
            let ok = crate::codegen::emit_typed_arg_guard(ctx.block(), *rep, value);
            guard = Some(match guard {
                Some(prev) => ctx.block().and(I1, &prev, &ok),
                None => ok,
            });
        }
        let fast_idx = ctx.new_block("typed_i32_call.fast");
        let fallback_idx = ctx.new_block("typed_i32_call.fallback");
        let merge_idx = ctx.new_block("typed_i32_call.merge");
        let fast_label = ctx.block_label(fast_idx);
        let fallback_label = ctx.block_label(fallback_idx);
        let merge_label = ctx.block_label(merge_idx);
        if let Some(guard) = guard {
            ctx.block().cond_br(&guard, &fast_label, &fallback_label);
        } else {
            ctx.block().br(&fast_label);
        }

        ctx.current_block = fast_idx;
        let mut typed_args_storage: Vec<String> = Vec::with_capacity(lowered.len());
        for (value, rep) in lowered.iter().zip(reps.iter()) {
            typed_args_storage.push(crate::codegen::emit_typed_arg_to_raw(
                ctx.block(),
                *rep,
                value,
            ));
        }
        let typed_args: Vec<(crate::types::LlvmType, &str)> = typed_args_storage
            .iter()
            .zip(reps.iter())
            .map(|(s, rep)| (rep.llvm_ty(), s.as_str()))
            .collect();
        let raw_i32 = ctx.block().call(I32, &typed_name, &typed_args);
        let fast_value = i32_to_nanbox(ctx.block(), &raw_i32);
        let after_fast = ctx.block().label.clone();
        if !ctx.block().is_terminated() {
            ctx.block().br(&merge_label);
        }

        ctx.current_block = fallback_idx;
        let fallback_value = ctx.block().call(DOUBLE, &generic_body_name, &arg_slices);
        let after_fallback = ctx.block().label.clone();
        if !ctx.block().is_terminated() {
            ctx.block().br(&merge_label);
        }

        ctx.current_block = merge_idx;
        let result = ctx.block().phi(
            DOUBLE,
            &[
                (fast_value.as_str(), after_fast.as_str()),
                (fallback_value.as_str(), after_fallback.as_str()),
            ],
        );
        ctx.record_lowered_value(
            "Call",
            None,
            "typed_i32_func_ref_call",
            &LoweredValue::js_value(result.clone()),
            None,
            None,
            None,
            false,
            false,
            vec![
                format!("typed_clone={typed_name}; generic_body={generic_body_name}"),
                typed_signature_note("i32", &reps, false),
                "boxed_result_at=direct_call_boundary".to_string(),
            ],
        );
        result
    } else if let Some(reps) = typed_string_call_param_reps {
        let typed_name = crate::codegen::typed_string_function_name(&fname);
        let generic_body_name = crate::codegen::generic_function_body_name(&fname);
        let mut guard: Option<String> = None;
        for (value, rep) in lowered.iter().zip(reps.iter()) {
            let ok = crate::codegen::emit_typed_arg_guard(ctx.block(), *rep, value);
            guard = Some(match guard {
                Some(prev) => ctx.block().and(I1, &prev, &ok),
                None => ok,
            });
        }
        let fast_idx = ctx.new_block("typed_string_call.fast");
        let fallback_idx = ctx.new_block("typed_string_call.fallback");
        let merge_idx = ctx.new_block("typed_string_call.merge");
        let fast_label = ctx.block_label(fast_idx);
        let fallback_label = ctx.block_label(fallback_idx);
        let merge_label = ctx.block_label(merge_idx);
        if let Some(guard) = guard {
            ctx.block().cond_br(&guard, &fast_label, &fallback_label);
        } else {
            ctx.block().br(&fast_label);
        }

        ctx.current_block = fast_idx;
        let mut typed_args_storage: Vec<String> = Vec::with_capacity(lowered.len());
        for (value, rep) in lowered.iter().zip(reps.iter()) {
            typed_args_storage.push(crate::codegen::emit_typed_arg_to_raw(
                ctx.block(),
                *rep,
                value,
            ));
        }
        let typed_args: Vec<(crate::types::LlvmType, &str)> = typed_args_storage
            .iter()
            .zip(reps.iter())
            .map(|(s, rep)| (rep.llvm_ty(), s.as_str()))
            .collect();
        let raw_string = ctx.block().call(I64, &typed_name, &typed_args);
        let fast_value = ctx
            .block()
            .call(DOUBLE, "js_nanbox_string", &[(I64, &raw_string)]);
        let after_fast = ctx.block().label.clone();
        if !ctx.block().is_terminated() {
            ctx.block().br(&merge_label);
        }

        ctx.current_block = fallback_idx;
        let fallback_value = ctx.block().call(DOUBLE, &generic_body_name, &arg_slices);
        let after_fallback = ctx.block().label.clone();
        if !ctx.block().is_terminated() {
            ctx.block().br(&merge_label);
        }

        ctx.current_block = merge_idx;
        let result = ctx.block().phi(
            DOUBLE,
            &[
                (fast_value.as_str(), after_fast.as_str()),
                (fallback_value.as_str(), after_fallback.as_str()),
            ],
        );
        ctx.record_lowered_value(
            "Call",
            None,
            "typed_string_func_ref_call",
            &LoweredValue::js_value(result.clone()),
            None,
            None,
            None,
            false,
            false,
            vec![
                format!("typed_clone={typed_name}; generic_body={generic_body_name}"),
                "typed_signature=string(i64, ...)->string".to_string(),
                "boxed_result_at=direct_call_boundary".to_string(),
            ],
        );
        result
    } else if let Some(typed_i1_param_reps) = typed_i1_call_param_reps {
        let typed_name = crate::codegen::typed_i1_function_name(&fname);
        let generic_body_name = crate::codegen::generic_function_body_name(&fname);
        let mut guard: Option<String> = None;
        for (value, rep) in lowered.iter().zip(typed_i1_param_reps.iter()) {
            let raw = ctx
                .block()
                .call(I32, rep.guard_fn(), &[(DOUBLE, value.as_str())]);
            let ok = ctx.block().icmp_ne(I32, &raw, "0");
            guard = Some(match guard {
                Some(prev) => ctx.block().and(I1, &prev, &ok),
                None => ok,
            });
        }
        let fast_idx = ctx.new_block("typed_i1_call.fast");
        let fallback_idx = ctx.new_block("typed_i1_call.fallback");
        let merge_idx = ctx.new_block("typed_i1_call.merge");
        let fast_label = ctx.block_label(fast_idx);
        let fallback_label = ctx.block_label(fallback_idx);
        let merge_label = ctx.block_label(merge_idx);
        if let Some(guard) = guard {
            ctx.block().cond_br(&guard, &fast_label, &fallback_label);
        } else {
            ctx.block().br(&fast_label);
        }

        ctx.current_block = fast_idx;
        let mut typed_args_storage: Vec<String> = Vec::with_capacity(lowered.len());
        for (value, rep) in lowered.iter().zip(typed_i1_param_reps.iter()) {
            typed_args_storage.push(match rep {
                crate::codegen::TypedParamRep::F64 => {
                    ctx.block()
                        .call(DOUBLE, rep.unbox_fn(), &[(DOUBLE, value.as_str())])
                }
                crate::codegen::TypedParamRep::I32 => {
                    ctx.block()
                        .call(I32, rep.unbox_fn(), &[(DOUBLE, value.as_str())])
                }
                crate::codegen::TypedParamRep::I1 => {
                    let raw_i32 =
                        ctx.block()
                            .call(I32, rep.unbox_fn(), &[(DOUBLE, value.as_str())]);
                    ctx.block().icmp_ne(I32, &raw_i32, "0")
                }
                crate::codegen::TypedParamRep::StringRef => {
                    ctx.block()
                        .call(I64, rep.unbox_fn(), &[(DOUBLE, value.as_str())])
                }
            });
        }
        let typed_args: Vec<(crate::types::LlvmType, &str)> = typed_args_storage
            .iter()
            .zip(typed_i1_param_reps.iter())
            .map(|(s, rep)| (rep.llvm_ty(), s.as_str()))
            .collect();
        let fast_i1 = ctx.block().call(I1, &typed_name, &typed_args);
        let fast_i32 = ctx.block().zext(I1, &fast_i1, I32);
        let fast_value = i32_bool_to_nanbox(ctx.block(), &fast_i32);
        let after_fast = ctx.block().label.clone();
        if !ctx.block().is_terminated() {
            ctx.block().br(&merge_label);
        }

        ctx.current_block = fallback_idx;
        let fallback_value = ctx.block().call(DOUBLE, &generic_body_name, &arg_slices);
        let after_fallback = ctx.block().label.clone();
        if !ctx.block().is_terminated() {
            ctx.block().br(&merge_label);
        }

        ctx.current_block = merge_idx;
        let result = ctx.block().phi(
            DOUBLE,
            &[
                (fast_value.as_str(), after_fast.as_str()),
                (fallback_value.as_str(), after_fallback.as_str()),
            ],
        );
        ctx.record_lowered_value(
            "Call",
            None,
            "typed_i1_func_ref_call",
            &LoweredValue::js_value(result.clone()),
            None,
            None,
            None,
            false,
            false,
            vec![
                format!("typed_clone={typed_name}; generic_body={generic_body_name}"),
                typed_i1_signature_note(&typed_i1_param_reps),
                "boxed_result_at=direct_call_boundary".to_string(),
            ],
        );
        result
    } else {
        let arena_body = crate::codegen::arena_threaded_function_body_name(&fname);
        if ctx.func.name == arena_body {
            let arena_slot = ctx
                .arena_state_slot
                .clone()
                .expect("arena-threaded body must cache its hidden parameter");
            let arena_state = ctx.block().load(PTR, &arena_slot);
            let mut recursive_args = arg_slices.clone();
            recursive_args.push((PTR, &arena_state));
            ctx.block().call(DOUBLE, &arena_body, &recursive_args)
        } else {
            ctx.block().call(DOUBLE, &fname, &arg_slices)
        }
    };
    // #7154: release the argument roots HERE and nowhere earlier.
    //
    // Every arm above either emits one call in the current block or splits into
    // a fast/fallback diamond and leaves `ctx.current_block` on the merge, so
    // this point post-dominates all five call sites. Below the call, because
    // the callee allocates while reading these arguments; after the diamond,
    // because releasing on one side of it would leave the other side's call
    // reading dropped slots.
    //
    // AFTER `implicit_this_restore`, and that order is load-bearing rather than
    // stylistic. `implicit_this_save` runs BELOW the argument lowering, so its
    // slot sits ABOVE this group, and `js_gc_temp_root_truncate` drops `base`
    // and everything above it. Releasing first therefore drops the saved
    // receiver, and `js_gc_temp_root_get` answers an out-of-range read with
    // `0` — so the restore would rebind the enclosing method's `this` to the
    // NUMBER 0. `implicit_this_restore` truncates at its own (higher) slot, and
    // its doc calls out that a caller holding a lower group may release
    // afterwards and drop the slot a second time harmlessly.
    if let Some(prev) = prev_this {
        crate::rooting::implicit_this_restore(ctx, prev);
    }
    arg_group.release(ctx);
    if ctx.local_generator_funcs.contains(fid) {
        let wrap_ptr = format!("@__perry_wrap_{}", fname);
        let closure_handle =
            ctx.block()
                .call(I64, "js_closure_alloc_singleton", &[(PTR, &wrap_ptr)]);
        return Ok(Some(ctx.block().call(
            DOUBLE,
            "js_generator_attach_closure_prototype",
            &[(DOUBLE, &result), (I64, &closure_handle)],
        )));
    }

    Ok(Some(result))
}
