use crate::native_value::artifact::{NativeAbiTransitionOp, NativeRepRecord, PodLayoutManifest};
use crate::native_value::pod::recompute_layout_from_fields;
use crate::native_value::rep::NativeRep;
use crate::types::{DOUBLE, F32, I1, I128, I32, I64, I8, PTR};

pub(crate) fn expected_llvm_type(rep: &NativeRep) -> Option<&'static str> {
    Some(match rep {
        NativeRep::JsValue | NativeRep::F64 => DOUBLE,
        NativeRep::I1 => I1,
        NativeRep::F32 => F32,
        NativeRep::JsValueBits
        | NativeRep::StringRef
        | NativeRep::I64
        | NativeRep::U64
        | NativeRep::USize
        | NativeRep::HandleId
        | NativeRep::NativeHandle
        | NativeRep::PromiseBoundary => I64,
        NativeRep::SmallBigInt => I128,
        NativeRep::I32 | NativeRep::U32 => I32,
        NativeRep::BufferLen => I32,
        NativeRep::U8 => I8,
        NativeRep::BufferView(_) => PTR,
        NativeRep::PodRecord { .. } => PTR,
        NativeRep::PodRecordView { .. } => PTR,
    })
}

pub(crate) fn validate_pod_layout(
    layout: &PodLayoutManifest,
    record: &NativeRepRecord,
    errors: &mut Vec<String>,
) {
    let prefix = || {
        format!(
            "{}:{} {}",
            record.function, record.block_label, record.consumer
        )
    };
    if layout.endian != "native" {
        errors.push(format!("{} pod layout has non-native endian", prefix()));
    }
    if layout.packing != "c" {
        errors.push(format!("{} pod layout has non-c packing", prefix()));
    }
    let has_nested_paths = layout.fields.iter().any(|field| field.path.len() > 1);
    let recomputed = if has_nested_paths {
        None
    } else {
        let specs: Vec<(String, NativeRep)> = layout
            .fields
            .iter()
            .map(|field| (field.name.clone(), field.native_rep.clone()))
            .collect();
        match recompute_layout_from_fields(layout.layout_id.clone(), &specs) {
            Ok(layout) => Some(layout),
            Err(reason) => {
                errors.push(format!(
                    "{} pod layout recompute failed: {}",
                    prefix(),
                    reason
                ));
                return;
            }
        }
    };
    if let Some(recomputed) = recomputed.as_ref() {
        if layout.size != recomputed.size || layout.alignment != recomputed.alignment {
            errors.push(format!(
                "{} pod layout size/alignment mismatch recorded=({},{}) recomputed=({},{})",
                prefix(),
                layout.size,
                layout.alignment,
                recomputed.size,
                recomputed.alignment
            ));
        }
        if layout.tail_padding != recomputed.tail_padding {
            errors.push(format!(
                "{} pod layout tail padding mismatch recorded={} recomputed={}",
                prefix(),
                layout.tail_padding,
                recomputed.tail_padding
            ));
        }
        if layout.padding != recomputed.padding {
            errors.push(format!("{} pod layout padding mismatch", prefix()));
        }
        if layout.fields.len() != recomputed.fields.len() {
            errors.push(format!("{} pod layout field count mismatch", prefix()));
            return;
        }
    }
    let mut ranges = Vec::with_capacity(layout.fields.len());
    for (idx, field) in layout.fields.iter().enumerate() {
        if field.path.is_empty() || field.name != field.path.join(".") {
            errors.push(format!(
                "{} pod field {} has invalid path",
                prefix(),
                field.name
            ));
        }
        if let Some(expected) = recomputed
            .as_ref()
            .and_then(|layout| layout.fields.get(idx))
        {
            if field.name != expected.name
                || field.native_rep != expected.native_rep
                || field.native_rep_name != field.native_rep.name()
                || field.offset != expected.offset
                || field.size != expected.size
                || field.alignment != expected.alignment
                || field.padding_before != expected.padding_before
            {
                errors.push(format!(
                    "{} pod field layout mismatch for {}",
                    prefix(),
                    field.name
                ));
            }
        } else if field.native_rep_name != field.native_rep.name() {
            errors.push(format!(
                "{} pod field {} native rep name mismatch",
                prefix(),
                field.name
            ));
        }
        if field.offset % field.alignment != 0 {
            errors.push(format!(
                "{} pod field {} offset {} violates alignment {}",
                prefix(),
                field.name,
                field.offset,
                field.alignment
            ));
        }
        ranges.push((
            field.offset,
            field.offset.saturating_add(field.size),
            &field.name,
        ));
    }
    ranges.sort_by_key(|(start, _, _)| *start);
    for pair in ranges.windows(2) {
        let (a_start, a_end, a_name) = pair[0];
        let (b_start, _, b_name) = pair[1];
        if a_end > b_start {
            errors.push(format!(
                "{} pod fields overlap: {}@{}..{} and {}@{}",
                prefix(),
                a_name,
                a_start,
                a_end,
                b_name,
                b_start
            ));
        }
    }
    let pointer_mask_nonzero = layout.pointer_mask.iter().any(|word| *word != 0);
    if pointer_mask_nonzero && !layout.explicit_pointer_metadata {
        errors.push(format!(
            "{} pod layout has nonzero pointer mask without explicit metadata",
            prefix()
        ));
    }
}

pub(crate) fn valid_native_abi_transition(
    from: &str,
    to: &str,
    op: &NativeAbiTransitionOp,
    lossy: bool,
    record_rep: &NativeRep,
) -> bool {
    if to == NativeRep::JsValueBits.name() {
        if !matches!(record_rep, NativeRep::JsValueBits) {
            return false;
        }
        return match op {
            NativeAbiTransitionOp::None => from == "f64" && !lossy,
            NativeAbiTransitionOp::JsValueToBits => from == "js_value" && !lossy,
            NativeAbiTransitionOp::BitsToJsValue => false,
            NativeAbiTransitionOp::SignedIntToFloat => {
                matches!(from, "i32" | "i64") && lossy == (from == "i64")
            }
            NativeAbiTransitionOp::UnsignedIntToFloat => {
                matches!(
                    from,
                    "u8" | "u32" | "u64" | "usize" | "buffer_len" | "handle_id"
                ) && lossy == matches!(from, "u64" | "usize" | "handle_id")
            }
            NativeAbiTransitionOp::FloatExtend => from == "f32" && !lossy,
            NativeAbiTransitionOp::PointerBox | NativeAbiTransitionOp::NativeHandleBox => {
                from == "native_handle" && !lossy
            }
            NativeAbiTransitionOp::PromiseBox => from == "promise_boundary" && !lossy,
            NativeAbiTransitionOp::BoolToJsValue => from == "i1" && !lossy,
            NativeAbiTransitionOp::BigIntBox => from == "small_bigint" && !lossy,
        };
    }
    if to != NativeRep::JsValue.name() {
        return false;
    }
    if !matches!(record_rep, NativeRep::JsValue) {
        return false;
    }
    match op {
        NativeAbiTransitionOp::None => matches!(from, "f64" | "js_value") && !lossy,
        NativeAbiTransitionOp::JsValueToBits => false,
        NativeAbiTransitionOp::BitsToJsValue => from == "js_value_bits" && !lossy,
        NativeAbiTransitionOp::SignedIntToFloat => {
            matches!(from, "i32" | "i64") && lossy == (from == "i64")
        }
        NativeAbiTransitionOp::UnsignedIntToFloat => {
            matches!(
                from,
                "u8" | "u32" | "u64" | "usize" | "buffer_len" | "handle_id"
            ) && lossy == matches!(from, "u64" | "usize" | "handle_id")
        }
        NativeAbiTransitionOp::FloatExtend => from == "f32" && !lossy,
        NativeAbiTransitionOp::PointerBox => {
            matches!(from, "native_handle" | "string_ref") && !lossy
        }
        NativeAbiTransitionOp::NativeHandleBox => from == "native_handle" && !lossy,
        NativeAbiTransitionOp::PromiseBox => from == "promise_boundary" && !lossy,
        NativeAbiTransitionOp::BoolToJsValue => from == "i1" && !lossy,
        NativeAbiTransitionOp::BigIntBox => from == "small_bigint" && !lossy,
    }
}
