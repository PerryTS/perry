use super::*;

fn bounds_proof_label(proof: &BoundsProof) -> &'static str {
    match proof {
        BoundsProof::LoopGuard => "loop_guard",
        BoundsProof::MinLength => "min_length",
        BoundsProof::ExplicitGuard => "explicit_guard",
        BoundsProof::ExplicitAssume => "explicit_assume",
    }
}

fn materialization_reason_label(reason: &MaterializationReason) -> &'static str {
    match reason {
        MaterializationReason::FunctionAbi => "function_abi",
        MaterializationReason::ReturnAbi => "return_abi",
        MaterializationReason::GenericCall => "generic_call",
        MaterializationReason::DynamicPropertyAccess => "dynamic_property_access",
        MaterializationReason::ExceptionPath => "exception_path",
        MaterializationReason::RuntimeApi => "runtime_api",
        MaterializationReason::DebugLogging => "debug_logging",
        MaterializationReason::UnknownAlias => "unknown_alias",
        MaterializationReason::UnknownBounds => "unknown_bounds",
        MaterializationReason::ClosureCapture => "closure_capture",
        MaterializationReason::Reassignment => "reassignment",
        MaterializationReason::UnknownCallEscape => "unknown_call_escape",
        MaterializationReason::UseAfterDispose => "use_after_dispose",
        MaterializationReason::EscapingUnownedPointer => "escaping_unowned_pointer",
        MaterializationReason::StaleViewLength => "stale_view_length",
        MaterializationReason::MutableAlias => "mutable_alias",
        MaterializationReason::MissingOwnerRoot => "missing_owner_root",
        MaterializationReason::PodMaterialization => "pod_materialization",
        MaterializationReason::PodUnsupported => "pod_unsupported",
        MaterializationReason::PodDynamicMutation => "pod_dynamic_mutation",
    }
}

fn native_fact_use(
    kind: &'static str,
    local_id: Option<u32>,
    state: &'static str,
    detail: &str,
    reason: Option<MaterializationReason>,
) -> NativeFactUse {
    let local = local_id
        .map(|id| id.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    NativeFactUse {
        fact_id: format!("native_region.{}.{}.{}", kind, local, detail),
        kind: kind.to_string(),
        local_id,
        state: state.to_string(),
        detail: detail.to_string(),
        reason,
    }
}

pub(crate) fn raw_f64_layout_fact(
    local_id: Option<u32>,
    state: &'static str,
    detail: &str,
    reason: Option<MaterializationReason>,
) -> NativeFactUse {
    native_fact_use("raw_f64_layout", local_id, state, detail, reason)
}

pub(crate) fn array_kind_fact(
    local_id: Option<u32>,
    state: &'static str,
    detail: &str,
    reason: Option<MaterializationReason>,
) -> NativeFactUse {
    native_fact_use("array_kind", local_id, state, detail, reason)
}

pub(crate) fn effect_fact(
    local_id: Option<u32>,
    state: &'static str,
    detail: &str,
    reason: Option<MaterializationReason>,
) -> NativeFactUse {
    native_fact_use("effect", local_id, state, detail, reason)
}

pub(super) fn native_fact_uses_for_record(
    local_id: Option<u32>,
    lowered: &LoweredValue,
    bounds_state: Option<&BoundsState>,
    alias_state: Option<&AliasState>,
    access_mode: Option<&BufferAccessMode>,
    materialization_reason: Option<&MaterializationReason>,
) -> (Vec<NativeFactUse>, Vec<NativeFactUse>) {
    let mut consumed = Vec::new();
    let mut rejected = Vec::new();
    match &lowered.rep {
        NativeRep::JsValueBits => consumed.push(native_fact_use(
            "representation",
            local_id,
            "consumed",
            "js_value_bits",
            None,
        )),
        NativeRep::JsValue => {}
        NativeRep::StringRef => consumed.push(native_fact_use(
            "representation",
            local_id,
            "consumed",
            "string_ref",
            None,
        )),
        NativeRep::I32 => consumed.push(native_fact_use(
            "representation",
            local_id,
            "consumed",
            "i32",
            None,
        )),
        NativeRep::I64 => consumed.push(native_fact_use(
            "representation",
            local_id,
            "consumed",
            "i64",
            None,
        )),
        NativeRep::U32 => consumed.push(native_fact_use(
            "representation",
            local_id,
            "consumed",
            "u32",
            None,
        )),
        NativeRep::U64 => consumed.push(native_fact_use(
            "representation",
            local_id,
            "consumed",
            "u64",
            None,
        )),
        NativeRep::USize => consumed.push(native_fact_use(
            "representation",
            local_id,
            "consumed",
            "usize",
            None,
        )),
        NativeRep::I1 => consumed.push(native_fact_use(
            "representation",
            local_id,
            "consumed",
            "i1",
            None,
        )),
        NativeRep::F64 => consumed.push(native_fact_use(
            "representation",
            local_id,
            "consumed",
            "f64",
            None,
        )),
        NativeRep::F32 => consumed.push(native_fact_use(
            "representation",
            local_id,
            "consumed",
            "f32",
            None,
        )),
        NativeRep::U8 => consumed.push(native_fact_use(
            "representation",
            local_id,
            "consumed",
            "u8",
            None,
        )),
        NativeRep::BufferLen => consumed.push(native_fact_use(
            "representation",
            local_id,
            "consumed",
            "buffer_len",
            None,
        )),
        NativeRep::HandleId => consumed.push(native_fact_use(
            "representation",
            local_id,
            "consumed",
            "handle_id",
            None,
        )),
        NativeRep::NativeHandle => consumed.push(native_fact_use(
            "representation",
            local_id,
            "consumed",
            "native_handle",
            None,
        )),
        NativeRep::PromiseBoundary => consumed.push(native_fact_use(
            "representation",
            local_id,
            "consumed",
            "promise_boundary",
            None,
        )),
        NativeRep::SmallBigInt => consumed.push(native_fact_use(
            "representation",
            local_id,
            "consumed",
            "small_bigint",
            None,
        )),
        NativeRep::BufferView(_) => consumed.push(native_fact_use(
            "representation",
            local_id,
            "consumed",
            "buffer_view",
            None,
        )),
        NativeRep::PodRecord { layout_id, .. } => consumed.push(native_fact_use(
            "representation",
            local_id,
            "consumed",
            layout_id,
            None,
        )),
        NativeRep::PodRecordView { layout_id, .. } => consumed.push(native_fact_use(
            "representation",
            local_id,
            "consumed",
            layout_id,
            None,
        )),
    }
    match bounds_state {
        Some(BoundsState::Proven { proof }) => consumed.push(native_fact_use(
            "bounds",
            local_id,
            "consumed",
            bounds_proof_label(proof),
            None,
        )),
        Some(BoundsState::Guarded { guard_id }) => consumed.push(native_fact_use(
            "bounds", local_id, "consumed", guard_id, None,
        )),
        Some(BoundsState::Unknown) | None => {
            if matches!(
                access_mode,
                Some(BufferAccessMode::DynamicFallback | BufferAccessMode::CheckedNative)
            ) {
                rejected.push(native_fact_use(
                    "bounds",
                    local_id,
                    "missing",
                    "unknown",
                    materialization_reason.cloned(),
                ));
            }
        }
    }
    match alias_state {
        Some(AliasState::NoAliasProven) => consumed.push(native_fact_use(
            "alias_noalias",
            local_id,
            "consumed",
            "noalias_proven",
            None,
        )),
        Some(AliasState::NoAliasGuarded { guard_id }) => consumed.push(native_fact_use(
            "alias_noalias",
            local_id,
            "consumed",
            guard_id,
            None,
        )),
        Some(AliasState::MayAlias | AliasState::Unknown) | None => {
            if matches!(access_mode, Some(BufferAccessMode::DynamicFallback)) {
                rejected.push(native_fact_use(
                    "alias_noalias",
                    local_id,
                    "missing",
                    "unknown_or_may_alias",
                    materialization_reason.cloned(),
                ));
            }
        }
    }
    if let Some(reason) = materialization_reason {
        rejected.push(native_fact_use(
            "materialization_hazard",
            local_id,
            "invalidated",
            materialization_reason_label(reason),
            Some(reason.clone()),
        ));
    }
    (consumed, rejected)
}
