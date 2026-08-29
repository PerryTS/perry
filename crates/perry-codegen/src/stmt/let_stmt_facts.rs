//! Local-fact helpers split out of `let_stmt.rs` (POD/native-rep provenance
//! strings, alias sources, scalar-class collection and the rejection/shape
//! diagnostics). Extracted to keep `let_stmt.rs` under the 2000-line cap;
//! no behavior change.

use super::*;

/// Remember an immutable `const n = receiver.length` association for guarded
/// counted-loop admission. The property read itself keeps ordinary semantics;
/// only a later runtime proof is allowed to consume this association.
pub(super) fn record_array_length_snapshot(ctx: &mut FnCtx<'_>, id: u32, init: &perry_hir::Expr) {
    if ctx.reassigned_locals.contains(&id) {
        return;
    }
    if let perry_hir::Expr::PropertyGet {
        object, property, ..
    } = init
    {
        if property == "length" {
            if let perry_hir::Expr::LocalGet(array_id) = object.as_ref() {
                ctx.array_length_snapshots.insert(id, *array_id);
            }
        }
    }
}

use crate::native_value::BufferAccessMode;

/// #8691: the aggregate scalar-replacement transform erases the carrier array
/// and object literals before codegen, leaving one synthetic local per field.
/// Preserve lowering evidence for those eliminated allocations so the result
/// remains visible to `--explain-lowering`.
pub(super) fn record_scalar_aggregate_field(ctx: &mut FnCtx<'_>, id: u32, name: &str, value: &str) {
    if !name.starts_with("__perry_scalar_aggregate_") {
        return;
    }
    let lowered = crate::native_value::LoweredValue {
        semantic: crate::native_value::SemanticKind::JsValue,
        rep: crate::native_value::NativeRep::JsValue,
        llvm_ty: DOUBLE,
        value: value.to_string(),
    };
    ctx.record_lowered_value_with_access_mode(
        "ScalarAggregateFieldInit",
        Some(id),
        "scalar_object_field_store",
        &lowered,
        None,
        None,
        None,
        None,
        false,
        false,
        vec![
            format!("local={name}"),
            "carrier_array=elided".to_string(),
            "carrier_object=elided".to_string(),
            "write_barrier=0".to_string(),
        ],
    );
}

pub(super) fn pod_view_count_source(ctx: &FnCtx<'_>, expr: &perry_hir::Expr) -> String {
    match expr {
        perry_hir::Expr::Integer(n) => format!("constant:{n}"),
        perry_hir::Expr::Number(n) if n.is_finite() && n.fract() == 0.0 => {
            format!("constant:{}", *n as i64)
        }
        perry_hir::Expr::LocalGet(id) => ctx
            .local_id_to_name
            .get(id)
            .map(|name| format!("local:{name}"))
            .unwrap_or_else(|| format!("local_id:{id}")),
        _ => "dynamic".to_string(),
    }
}

pub(super) fn native_i32_alias_source(expr: &perry_hir::Expr) -> Option<u32> {
    match expr {
        perry_hir::Expr::Binary {
            op: perry_hir::BinaryOp::BitOr,
            left,
            right,
        } if matches!(right.as_ref(), perry_hir::Expr::Integer(0)) => match left.as_ref() {
            perry_hir::Expr::LocalGet(id) => Some(*id),
            _ => native_i32_alias_source(left),
        },
        perry_hir::Expr::LocalGet(id) => Some(*id),
        _ => None,
    }
}

pub(super) fn buffer_local_alias_source(expr: &perry_hir::Expr) -> Option<u32> {
    match expr {
        perry_hir::Expr::LocalGet(id) => Some(*id),
        _ => None,
    }
}

/// Extract all field names (parent chain + own) and the constructor for
/// a class, cloning everything out of `ctx.classes` so the immutable
/// borrow is released before the caller mutates `ctx`.
///
/// Returns `None` if the class is not found in `ctx.classes`.
pub(crate) fn collect_scalar_class_data(
    ctx: &FnCtx<'_>,
    class_name: &str,
) -> Option<(Vec<String>, Option<perry_hir::Function>)> {
    let class = ctx.classes.get(class_name)?;
    let mut all_fields: Vec<String> = Vec::new();
    let mut chain: Vec<String> = Vec::new();
    let mut p = class.extends_name.clone();
    while let Some(pname) = p {
        chain.push(pname.clone());
        if let Some(pc) = ctx.classes.get(pname.as_str()) {
            p = pc.extends_name.clone();
        } else {
            break;
        }
    }
    chain.reverse();
    for pname in &chain {
        if let Some(pc) = ctx.classes.get(pname.as_str()) {
            for f in &pc.fields {
                all_fields.push(f.name.clone());
            }
        }
    }
    for f in &class.fields {
        all_fields.push(f.name.clone());
    }
    let ctor = class.constructor.clone();
    Some((all_fields, ctor))
}

pub(super) fn record_pod_rejection(ctx: &mut FnCtx<'_>, id: u32, reason: String) {
    let undef = crate::nanbox::double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED));
    let lowered = LoweredValue::js_value(undef);
    ctx.record_lowered_value_with_access_mode(
        "PodRecordRejected",
        Some(id),
        "pod_record_fallback_to_js_object",
        &lowered,
        None,
        None,
        Some(BufferAccessMode::DynamicFallback),
        Some(MaterializationReason::PodUnsupported),
        false,
        false,
        vec![format!("reason={}", reason)],
    );
}

/// #7106 follow-up: record that a `Ptr<Shape>`-proven local was scalar-replaced,
/// so its promotion can never be consumed.
///
/// Report-only; the caller has already gated on `opt_report::enabled()`. The
/// fact is read through the context-free accessor on purpose — whether the
/// enclosing body would have ALLOWED consumption is a different mechanism with
/// a different rule name, and a value can lose to both.
pub(super) fn note_ptr_shape_scalar_replaced(ctx: &crate::expr::FnCtx<'_>, id: u32, name: &str) {
    let Some(fact) = ctx.native_facts.shape_proven_ptr_local(id) else {
        return;
    };
    let (reason, issue) =
        crate::expr::ptr_shape_context_rule_text(crate::expr::PTR_SHAPE_SCALAR_REPLACED);
    crate::opt_report::unconsumed(crate::opt_report::Unconsumed {
        position: crate::opt_report::Position::Local,
        name,
        local_id: Some(id),
        analysis: crate::opt_report::Analysis::PtrShape,
        rep: "Ptr<Shape>",
        rule: crate::expr::PTR_SHAPE_SCALAR_REPLACED,
        reason,
        tier: crate::opt_report::Tier::CompilerLimitation,
        issue: Some(issue),
        detail: Some(format!(
            "class {} scalar-replaced into per-field allocas; the allocation is gone",
            fact.class_name
        )),
    });
}

/// `globalThis` (or `globalThis.globalThis`) as an init value — moved here
/// from `let_stmt.rs` for the 2000-line cap; no behavior change.
pub(super) fn is_global_this_value(expr: &perry_hir::Expr) -> bool {
    matches!(expr, perry_hir::Expr::GlobalGet(_))
        || matches!(
            expr,
            perry_hir::Expr::PropertyGet { object, property, .. }
                if matches!(object.as_ref(), perry_hir::Expr::GlobalGet(_))
                    && property == "globalThis"
        )
}
