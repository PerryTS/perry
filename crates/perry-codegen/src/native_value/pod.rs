use perry_api_manifest::{NativeAbiType, NativePodAbi};
use perry_hir::types::{ObjectType, Type};
use perry_hir::Expr;

use crate::expr::FnCtx;
use crate::types::{DOUBLE, F32, I32, I64};

use super::artifact::{PodLayoutField, PodLayoutManifest, PodLayoutPadding};
use super::rep::{ExpectedNativeRep, NativeRep};

#[derive(Debug, Clone)]
pub(crate) struct PodLocal {
    pub layout: PodLayoutManifest,
    pub data_slot: String,
    pub materialized_slot: String,
}

#[derive(Debug, Clone)]
pub(crate) struct PodViewLocal {
    pub layout: PodLayoutManifest,
    pub view_slot: String,
    pub count_source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PodLayoutDecision {
    NotPod,
    Layout(PodLayoutManifest),
    Rejected(String),
}

#[derive(Debug, Clone)]
pub(crate) struct PodInitFields {
    pub fields: Vec<(String, Expr)>,
}

pub(crate) fn layout_decision_for_type(ctx: &FnCtx<'_>, ty: &Type) -> PodLayoutDecision {
    match ty {
        Type::Generic { base, type_args } if base == "PerryPod" => {
            if type_args.len() != 1 {
                return PodLayoutDecision::Rejected("pod_marker_requires_one_type_arg".to_string());
            }
            match layout_for_inner_type(ctx, &type_args[0], 0) {
                Ok(layout) => PodLayoutDecision::Layout(layout),
                Err(reason) => PodLayoutDecision::Rejected(reason),
            }
        }
        Type::Named(name) => {
            if let Some(alias) = ctx.type_aliases.get(name) {
                layout_decision_for_type(ctx, alias)
            } else {
                PodLayoutDecision::NotPod
            }
        }
        _ => PodLayoutDecision::NotPod,
    }
}

pub(crate) fn layout_for_pod_view_type(
    ctx: &FnCtx<'_>,
    ty: &Type,
) -> Result<PodLayoutManifest, String> {
    match ty {
        Type::Generic { base, type_args } if base == "PerryPodView" => {
            if type_args.len() != 1 {
                return Err("pod_view_marker_requires_one_type_arg".to_string());
            }
            match layout_decision_for_type(ctx, &type_args[0]) {
                PodLayoutDecision::Layout(layout) => Ok(layout),
                PodLayoutDecision::Rejected(reason) => {
                    Err(format!("pod_view_type_arg_rejected:{reason}"))
                }
                PodLayoutDecision::NotPod => {
                    Err("pod_view_type_arg_must_resolve_to_perry_pod".to_string())
                }
            }
        }
        Type::Named(name) => {
            if let Some(alias) = ctx.type_aliases.get(name) {
                layout_for_pod_view_type(ctx, alias)
            } else {
                Err("pod_view_requires_explicit_perry_pod_view_annotation".to_string())
            }
        }
        _ => Err("pod_view_requires_explicit_perry_pod_view_annotation".to_string()),
    }
}

pub(crate) fn collect_pod_init_fields(
    ctx: &FnCtx<'_>,
    init: &Expr,
) -> Result<PodInitFields, String> {
    match init {
        Expr::Object(props) => {
            let mut seen = std::collections::HashSet::new();
            let mut fields = Vec::with_capacity(props.len());
            for (name, value) in props {
                if !seen.insert(name.clone()) {
                    return Err(format!("duplicate_field:{}", name));
                }
                fields.push((name.clone(), value.clone()));
            }
            Ok(PodInitFields { fields })
        }
        Expr::New {
            class_name, args, ..
        } if class_name.starts_with("__AnonShape_") => {
            let class = ctx
                .classes
                .get(class_name)
                .ok_or_else(|| "anonymous_shape_class_missing".to_string())?;
            if class.extends.is_some()
                || class.extends_name.is_some()
                || class.native_extends.is_some()
                || class.extends_expr.is_some()
            {
                return Err("inherited_fields".to_string());
            }
            if class.fields.len() != args.len() {
                return Err("shape_field_arg_mismatch".to_string());
            }
            let mut fields = Vec::with_capacity(args.len());
            for (field, arg) in class.fields.iter().zip(args.iter()) {
                if field.key_expr.is_some() || field.is_private || !field.decorators.is_empty() {
                    return Err(format!("unsupported_field:{}", field.name));
                }
                fields.push((field.name.clone(), arg.clone()));
            }
            Ok(PodInitFields { fields })
        }
        Expr::ObjectSpread { .. } => Err("spread_property".to_string()),
        _ => Err("unsupported_initializer".to_string()),
    }
}

pub(crate) fn validate_exact_init(
    layout: &PodLayoutManifest,
    init_fields: &PodInitFields,
) -> Result<(), String> {
    if init_fields.fields.len() != layout.fields.len() {
        return Err(format!(
            "field_count_mismatch:expected={},actual={}",
            layout.fields.len(),
            init_fields.fields.len()
        ));
    }
    let mut seen = std::collections::HashSet::new();
    for ((actual_name, value), expected) in init_fields.fields.iter().zip(layout.fields.iter()) {
        if !seen.insert(actual_name.as_str()) {
            return Err(format!("duplicate_field:{}", actual_name));
        }
        if actual_name != &expected.name {
            return Err(format!(
                "field_order_or_name_mismatch:expected={},actual={}",
                expected.name, actual_name
            ));
        }
        if !pod_init_value_roundtrips_exact(&expected.native_rep, value) {
            return Err(format!(
                "field:{}:inexact_or_dynamic_initializer:{}",
                expected.name,
                expected.native_rep.name()
            ));
        }
    }
    Ok(())
}

fn pod_init_value_roundtrips_exact(rep: &NativeRep, value: &Expr) -> bool {
    match rep {
        NativeRep::I32 => {
            literal_i64(value).is_some_and(|n| i32::try_from(n).is_ok())
                || literal_f64(value).is_some_and(|n| {
                    int_roundtrips_exact(n, i32::MIN as f64, (i32::MAX as f64) + 1.0)
                })
        }
        NativeRep::I64 => {
            literal_i64(value).is_some_and(|n| {
                let as_f64 = n as f64;
                int_roundtrips_exact(as_f64, i64::MIN as f64, 9_223_372_036_854_775_808.0)
                    && (as_f64 as i64) == n
            }) || literal_f64(value).is_some_and(|n| {
                int_roundtrips_exact(n, i64::MIN as f64, 9_223_372_036_854_775_808.0)
            })
        }
        NativeRep::U32 | NativeRep::BufferLen => {
            literal_i64(value).is_some_and(|n| u32::try_from(n).is_ok())
                || literal_f64(value).is_some_and(|n| uint_roundtrips_exact(n, 4_294_967_296.0))
        }
        NativeRep::U64 | NativeRep::USize | NativeRep::HandleId => {
            literal_i64(value).is_some_and(|n| {
                if n < 0 {
                    return false;
                }
                let as_f64 = n as f64;
                uint_roundtrips_exact(as_f64, 18_446_744_073_709_551_616.0)
                    && (as_f64 as u64) == n as u64
            }) || literal_f64(value)
                .is_some_and(|n| uint_roundtrips_exact(n, 18_446_744_073_709_551_616.0))
        }
        NativeRep::F64 => matches!(value, Expr::Integer(_) | Expr::Number(_)),
        NativeRep::F32 => literal_f64(value).is_some_and(f32_roundtrips_exact),
        _ => false,
    }
}

fn literal_i64(value: &Expr) -> Option<i64> {
    match value {
        Expr::Integer(n) => Some(*n),
        _ => None,
    }
}

fn literal_f64(value: &Expr) -> Option<f64> {
    match value {
        Expr::Integer(n) => Some(*n as f64),
        Expr::Number(n) => Some(*n),
        _ => None,
    }
}

fn int_roundtrips_exact(number: f64, min_inclusive: f64, max_exclusive: f64) -> bool {
    number.is_finite()
        && number >= min_inclusive
        && number < max_exclusive
        && number.trunc() == number
        && !(number == 0.0 && number.is_sign_negative())
}

fn uint_roundtrips_exact(number: f64, max_exclusive: f64) -> bool {
    number.is_finite()
        && number >= 0.0
        && number < max_exclusive
        && number.trunc() == number
        && !(number == 0.0 && number.is_sign_negative())
}

fn f32_roundtrips_exact(number: f64) -> bool {
    !number.is_nan() && ((number as f32) as f64).to_bits() == number.to_bits()
}

pub(crate) fn field_expected_rep(field: &PodLayoutField) -> ExpectedNativeRep {
    expected_rep_for_native_rep(&field.native_rep).expect("pod layout contains scalar field reps")
}

pub(crate) fn llvm_type_for_native_rep(rep: &NativeRep) -> Option<&'static str> {
    Some(match rep {
        NativeRep::JsValue | NativeRep::F64 => DOUBLE,
        NativeRep::F32 => F32,
        NativeRep::I64 | NativeRep::U64 | NativeRep::USize | NativeRep::HandleId => I64,
        NativeRep::I32 | NativeRep::U32 | NativeRep::BufferLen => I32,
        _ => return None,
    })
}

pub(crate) fn expected_rep_for_native_rep(rep: &NativeRep) -> Option<ExpectedNativeRep> {
    Some(match rep {
        NativeRep::I32 => ExpectedNativeRep::I32,
        NativeRep::I64 => ExpectedNativeRep::I64,
        NativeRep::U32 => ExpectedNativeRep::U32,
        NativeRep::U64 => ExpectedNativeRep::U64,
        NativeRep::USize => ExpectedNativeRep::USize,
        NativeRep::F64 => ExpectedNativeRep::F64,
        NativeRep::F32 => ExpectedNativeRep::F32,
        NativeRep::BufferLen => ExpectedNativeRep::BufferLen,
        NativeRep::HandleId => ExpectedNativeRep::HandleId,
        _ => return None,
    })
}

pub(crate) fn native_rep_for_pod_abi_type(ty: &NativeAbiType) -> Option<NativeRep> {
    Some(match ty {
        NativeAbiType::I32 => NativeRep::I32,
        NativeAbiType::I64 => NativeRep::I64,
        NativeAbiType::U32 => NativeRep::U32,
        NativeAbiType::U64 => NativeRep::U64,
        NativeAbiType::USize => NativeRep::USize,
        NativeAbiType::F32 => NativeRep::F32,
        NativeAbiType::F64 => NativeRep::F64,
        NativeAbiType::BufferLen => NativeRep::BufferLen,
        NativeAbiType::HandleId => NativeRep::HandleId,
        _ => return None,
    })
}

pub(crate) fn layout_for_manifest_pod(pod: &NativePodAbi) -> Result<PodLayoutManifest, String> {
    layout_for_manifest_pod_with_prefix(pod, Vec::new(), 0)
}

fn layout_for_manifest_pod_with_prefix(
    pod: &NativePodAbi,
    prefix: Vec<String>,
    depth: u8,
) -> Result<PodLayoutManifest, String> {
    if depth > 8 {
        return Err("pod_descriptor_nesting_depth_limit".to_string());
    }
    if pod.fields.is_empty() {
        return Err("pod_descriptor_empty_fields".to_string());
    }
    let mut specs = Vec::with_capacity(pod.fields.len());
    for field in &pod.fields {
        if field.name.trim().is_empty() {
            return Err("pod_descriptor_empty_field_name".to_string());
        }
        let mut path = prefix.clone();
        path.push(field.name.clone());
        match &field.ty {
            NativeAbiType::Pod(nested) => {
                let layout = layout_for_manifest_pod_with_prefix(nested, Vec::new(), depth + 1)
                    .map_err(|reason| format!("field:{}:{}", field.name, reason))?;
                specs.push(PodFieldSpec::Nested {
                    name: field.name.clone(),
                    path: {
                        let mut p = prefix.clone();
                        p.push(field.name.clone());
                        p
                    },
                    layout,
                });
            }
            ty => {
                let rep = native_rep_for_pod_abi_type(ty)
                    .ok_or_else(|| format!("unsupported_pod_field_type:{}", ty))?;
                specs.push(PodFieldSpec::Scalar {
                    name: path.join("."),
                    path,
                    rep,
                });
            }
        }
    }
    compute_layout_from_specs(&specs)
}

pub(crate) fn scalar_size_align(rep: &NativeRep) -> Option<(u32, u32)> {
    Some(match rep {
        NativeRep::I32 | NativeRep::U32 | NativeRep::F32 | NativeRep::BufferLen => (4, 4),
        NativeRep::I64
        | NativeRep::U64
        | NativeRep::USize
        | NativeRep::F64
        | NativeRep::HandleId => (8, 8),
        _ => return None,
    })
}

#[derive(Debug, Clone)]
enum PodFieldSpec {
    Scalar {
        name: String,
        path: Vec<String>,
        rep: NativeRep,
    },
    Nested {
        name: String,
        path: Vec<String>,
        layout: PodLayoutManifest,
    },
}

pub(crate) fn recompute_layout_from_fields(
    layout_id: String,
    field_specs: &[(String, NativeRep)],
) -> Result<PodLayoutManifest, String> {
    let specs: Vec<PodFieldSpec> = field_specs
        .iter()
        .map(|(name, rep)| PodFieldSpec::Scalar {
            name: name.clone(),
            path: vec![name.clone()],
            rep: rep.clone(),
        })
        .collect();
    let mut layout = compute_layout_from_specs(&specs)?;
    layout.layout_id = layout_id;
    Ok(layout)
}

fn compute_layout_from_specs(field_specs: &[PodFieldSpec]) -> Result<PodLayoutManifest, String> {
    let mut fields = Vec::with_capacity(field_specs.len());
    let mut padding = Vec::new();
    let mut offset = 0u32;
    let mut max_align = 1u32;
    let mut has_f32 = false;

    for spec in field_specs {
        match spec {
            PodFieldSpec::Scalar { name, path, rep } => {
                has_f32 |= matches!(rep, NativeRep::F32);
                let (size, alignment) = scalar_size_align(rep)
                    .ok_or_else(|| format!("unsupported_field_rep:{}", rep.name()))?;
                max_align = max_align.max(alignment);
                let aligned = align_to(offset, alignment);
                let padding_before = aligned - offset;
                if padding_before != 0 {
                    padding.push(PodLayoutPadding {
                        offset,
                        size: padding_before,
                        reason: format!("align_field:{}", name),
                    });
                }
                fields.push(PodLayoutField {
                    name: name.clone(),
                    path: path.clone(),
                    native_rep: rep.clone(),
                    native_rep_name: rep.name().to_string(),
                    offset: aligned,
                    size,
                    alignment,
                    padding_before,
                });
                offset = aligned
                    .checked_add(size)
                    .ok_or_else(|| "pod_layout_size_overflow".to_string())?;
            }
            PodFieldSpec::Nested { name, path, layout } => {
                has_f32 |= layout
                    .fields
                    .iter()
                    .any(|field| matches!(field.native_rep, NativeRep::F32));
                max_align = max_align.max(layout.alignment);
                let aligned = align_to(offset, layout.alignment);
                let padding_before = aligned - offset;
                if padding_before != 0 {
                    padding.push(PodLayoutPadding {
                        offset,
                        size: padding_before,
                        reason: format!("align_field:{}", name),
                    });
                }
                for nested_padding in &layout.padding {
                    padding.push(PodLayoutPadding {
                        offset: aligned
                            .checked_add(nested_padding.offset)
                            .ok_or_else(|| "pod_layout_size_overflow".to_string())?,
                        size: nested_padding.size,
                        reason: format!("nested:{}:{}", name, nested_padding.reason),
                    });
                }
                for nested in &layout.fields {
                    let mut nested_path = path.clone();
                    if nested.path.is_empty() {
                        nested_path.push(nested.name.clone());
                    } else {
                        nested_path.extend(nested.path.clone());
                    }
                    fields.push(PodLayoutField {
                        name: nested_path.join("."),
                        path: nested_path,
                        native_rep: nested.native_rep.clone(),
                        native_rep_name: nested.native_rep_name.clone(),
                        offset: aligned
                            .checked_add(nested.offset)
                            .ok_or_else(|| "pod_layout_size_overflow".to_string())?,
                        size: nested.size,
                        alignment: nested.alignment,
                        padding_before: nested.padding_before,
                    });
                }
                offset = aligned
                    .checked_add(layout.size)
                    .ok_or_else(|| "pod_layout_size_overflow".to_string())?;
            }
        }
    }

    let final_size = align_to(offset, max_align);
    let tail_padding = final_size - offset;
    if tail_padding != 0 {
        padding.push(PodLayoutPadding {
            offset,
            size: tail_padding,
            reason: "tail_padding".to_string(),
        });
    }

    let mut materialization_hazards = vec![
        "dynamic_escape_materializes_to_plain_js_object".to_string(),
        "identity_observable_use_requires_materialization".to_string(),
    ];
    if has_f32 {
        materialization_hazards.push("f32_fields_widen_to_js_f64_on_materialization".to_string());
    }

    let mut layout = PodLayoutManifest {
        layout_id: String::new(),
        size: final_size,
        alignment: max_align,
        endian: "native".to_string(),
        packing: "c".to_string(),
        fields,
        padding,
        tail_padding,
        pointer_mask: Vec::new(),
        materialization_hazards,
        explicit_pointer_metadata: false,
    };
    layout.layout_id = compute_layout_id_from_layout(&layout);
    Ok(layout)
}

fn compute_layout_id_from_layout(layout: &PodLayoutManifest) -> String {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    fn mix(h: &mut u64, bytes: &[u8]) {
        for b in bytes {
            *h ^= *b as u64;
            *h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    mix(&mut h, b"perry_pod_v2\0");
    mix(&mut h, &layout.size.to_le_bytes());
    mix(&mut h, &layout.alignment.to_le_bytes());
    for field in &layout.fields {
        mix(&mut h, field.name.as_bytes());
        mix(&mut h, b":");
        mix(&mut h, field.native_rep.name().as_bytes());
        mix(&mut h, b"@");
        mix(&mut h, &field.offset.to_le_bytes());
        mix(&mut h, b"/");
        mix(&mut h, &field.size.to_le_bytes());
        mix(&mut h, b";");
    }
    format!("pod_{:016x}", h)
}

pub(crate) fn layout_runtime_id(layout_id: &str) -> u64 {
    layout_id
        .strip_prefix("pod_")
        .and_then(|hex| u64::from_str_radix(hex, 16).ok())
        .unwrap_or(0)
}

fn layout_for_inner_type(
    ctx: &FnCtx<'_>,
    ty: &Type,
    depth: u8,
) -> Result<PodLayoutManifest, String> {
    if depth > 8 {
        return Err("type_alias_cycle_or_depth_limit".to_string());
    }
    match ty {
        Type::Named(name) => {
            if let Some(alias) = ctx.type_aliases.get(name) {
                return layout_for_inner_type(ctx, alias, depth + 1);
            }
            if let Some(interface) = ctx.interfaces.get(name) {
                if !interface.extends.is_empty() {
                    return Err("inherited_fields".to_string());
                }
                if !interface.methods.is_empty() {
                    return Err("method_signature".to_string());
                }
                let mut specs = Vec::with_capacity(interface.properties.len());
                for property in &interface.properties {
                    if property.optional {
                        return Err(format!("optional_field:{}", property.name));
                    }
                    let spec = field_spec_from_type(
                        ctx,
                        property.name.clone(),
                        vec![property.name.clone()],
                        &property.ty,
                        depth + 1,
                    )
                    .map_err(|reason| format!("field:{}:{}", property.name, reason))?;
                    specs.push(spec);
                }
                return compute_layout_from_specs(&specs);
            }
            Err(format!("unknown_pod_type:{}", name))
        }
        Type::Object(object) => layout_for_object_type(ctx, object, depth),
        _ => Err("pod_inner_must_be_closed_object_or_interface".to_string()),
    }
}

fn layout_for_object_type(
    ctx: &FnCtx<'_>,
    object: &ObjectType,
    depth: u8,
) -> Result<PodLayoutManifest, String> {
    if object.index_signature.is_some() {
        return Err("index_signature".to_string());
    }
    let order = object
        .property_order
        .as_ref()
        .ok_or_else(|| "missing_property_order".to_string())?;
    if order.len() != object.properties.len() {
        return Err("property_order_mismatch".to_string());
    }
    let mut specs = Vec::with_capacity(order.len());
    let mut seen = std::collections::HashSet::new();
    for name in order {
        if !seen.insert(name.as_str()) {
            return Err(format!("duplicate_field:{}", name));
        }
        let property = object
            .properties
            .get(name)
            .ok_or_else(|| format!("missing_ordered_field:{}", name))?;
        if property.optional {
            return Err(format!("optional_field:{}", name));
        }
        let spec = field_spec_from_type(
            ctx,
            name.clone(),
            vec![name.clone()],
            &property.ty,
            depth + 1,
        )
        .map_err(|reason| format!("field:{}:{}", name, reason))?;
        specs.push(spec);
    }
    compute_layout_from_specs(&specs)
}

fn field_spec_from_type(
    ctx: &FnCtx<'_>,
    name: String,
    path: Vec<String>,
    ty: &Type,
    depth: u8,
) -> Result<PodFieldSpec, String> {
    if depth > 8 {
        return Err("type_alias_cycle_or_depth_limit".to_string());
    }
    match ty {
        Type::Generic { base, type_args } if base == "PerryPod" => {
            if type_args.len() != 1 {
                return Err("nested_pod_requires_one_type_arg".to_string());
            }
            let layout = layout_for_inner_type(ctx, &type_args[0], depth + 1)?;
            Ok(PodFieldSpec::Nested { name, path, layout })
        }
        Type::Named(named) => {
            if let Some(alias) = ctx.type_aliases.get(named) {
                if matches!(alias, Type::Generic { base, .. } if base == "PerryPod") {
                    field_spec_from_type(ctx, name, path, alias, depth + 1)
                } else {
                    let rep = field_native_rep(ctx, ty, depth)?;
                    Ok(PodFieldSpec::Scalar { name, path, rep })
                }
            } else {
                let rep = field_native_rep(ctx, ty, depth)?;
                Ok(PodFieldSpec::Scalar { name, path, rep })
            }
        }
        _ => {
            let rep = field_native_rep(ctx, ty, depth)?;
            Ok(PodFieldSpec::Scalar { name, path, rep })
        }
    }
}

fn field_native_rep(ctx: &FnCtx<'_>, ty: &Type, depth: u8) -> Result<NativeRep, String> {
    if depth > 8 {
        return Err("type_alias_cycle_or_depth_limit".to_string());
    }
    match ty {
        Type::Named(name) => match name.as_str() {
            "PerryU32" => Ok(NativeRep::U32),
            "PerryU64" => Ok(NativeRep::U64),
            "PerryUSize" => Ok(NativeRep::USize),
            "PerryF32" => Ok(NativeRep::F32),
            "PerryF64" => Ok(NativeRep::F64),
            "PerryI32" => Ok(NativeRep::I32),
            "PerryI64" => Ok(NativeRep::I64),
            "PerryBufferLen" => Ok(NativeRep::BufferLen),
            "PerryHandleId" => Ok(NativeRep::HandleId),
            other => {
                if let Some(alias) = ctx.type_aliases.get(other) {
                    field_native_rep(ctx, alias, depth + 1)
                } else {
                    Err("pointerful_field_without_metadata".to_string())
                }
            }
        },
        Type::Number => Ok(NativeRep::F64),
        _ => Err("unsupported_or_pointerful_field".to_string()),
    }
}

fn align_to(value: u32, alignment: u32) -> u32 {
    debug_assert!(alignment.is_power_of_two());
    (value + alignment - 1) & !(alignment - 1)
}
