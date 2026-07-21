use crate::native_value::artifact::{NativeAbiDirection, NativeRepRecord};
use crate::native_value::rep::NativeRep;

pub(crate) fn validate_native_abi_type_record(
    record: &NativeRepRecord,
    abi: &crate::native_value::artifact::NativeAbiTypeRecord,
    errors: &mut Vec<String>,
) {
    let prefix = || {
        format!(
            "{}:{} {}",
            record.function, record.block_label, record.consumer
        )
    };
    if abi.display.is_empty() || abi.canonical_kind.is_empty() {
        errors.push(format!("{} native ABI descriptor is empty", prefix()));
    }
    match abi.direction {
        NativeAbiDirection::Param => {
            if abi.js_argument_index.is_none() {
                errors.push(format!(
                    "{} native ABI param missing JS argument index",
                    prefix()
                ));
            }
        }
        NativeAbiDirection::Return => {
            if abi.js_argument_index.is_some() {
                errors.push(format!(
                    "{} native ABI return must not carry JS argument index",
                    prefix()
                ));
            }
            if abi.canonical_kind == "buffer+len" {
                errors.push(format!("{} buffer+len cannot be a return type", prefix()));
            }
            if abi.canonical_kind == "pod" {
                errors.push(format!("{} pod cannot be a return type", prefix()));
            }
            if abi.canonical_kind == "pod+count" {
                errors.push(format!("{} pod+count cannot be a return type", prefix()));
            }
        }
    }
    if abi.abi_slot_count == 0 && abi.canonical_kind != "void" {
        errors.push(format!("{} native ABI slot count is zero", prefix()));
    }
    validate_native_abi_runtime_guard(record, abi, errors);
    if abi.canonical_kind == "pod" {
        if abi.pod_fields.is_empty() {
            errors.push(format!("{} pod ABI missing field contract", prefix()));
        }
        if record.pod_layout.is_none() {
            errors.push(format!("{} pod ABI missing verifier layout", prefix()));
        }
        if let Some(layout) = record.pod_layout.as_ref() {
            if abi.pod_fields.len() != layout.fields.len() {
                errors.push(format!(
                    "{} pod ABI field count mismatches layout",
                    prefix()
                ));
            } else {
                for (abi_field, layout_field) in abi.pod_fields.iter().zip(layout.fields.iter()) {
                    if abi_field.name != layout_field.name
                        || abi_field.ty != layout_field.native_rep_name
                    {
                        errors.push(format!(
                            "{} pod ABI field {} does not match verifier layout",
                            prefix(),
                            abi_field.name
                        ));
                    }
                }
            }
        }
    }
    if abi.canonical_kind == "pod+count" {
        if abi.abi_slot_count != 2 {
            errors.push(format!("{} pod+count ABI must use two slots", prefix()));
        }
        if abi.pod_fields.is_empty() {
            errors.push(format!("{} pod+count ABI missing field contract", prefix()));
        }
        if record.pod_layout.is_none() {
            errors.push(format!(
                "{} pod+count ABI missing verifier layout",
                prefix()
            ));
        }
        if record.pod_record_view.is_none() {
            errors.push(format!("{} pod+count ABI missing pod view proof", prefix()));
        }
    }
    if abi.canonical_kind == "handle" {
        match abi.native_handle.as_ref() {
            Some(handle) => {
                if handle.direction != abi.direction
                    || handle.js_argument_index != abi.js_argument_index
                    || handle.abi_slot_index != abi.abi_slot_index
                    || handle.abi_slot_count != abi.abi_slot_count
                {
                    errors.push(format!(
                        "{} native handle contract slot metadata does not match ABI record",
                        prefix()
                    ));
                }
                if handle.type_id == 0 {
                    errors.push(format!("{} native handle type id is zero", prefix()));
                }
                if handle.debug_name.is_empty() {
                    errors.push(format!("{} native handle debug name is empty", prefix()));
                }
                if !matches!(handle.ownership.as_str(), "owned" | "borrowed") {
                    errors.push(format!("{} native handle ownership is invalid", prefix()));
                }
                if !matches!(handle.thread_affinity.as_str(), "any" | "main" | "creator") {
                    errors.push(format!(
                        "{} native handle thread affinity is invalid",
                        prefix()
                    ));
                }
                if handle.has_finalizer != handle.finalizer_symbol.is_some() {
                    errors.push(format!(
                        "{} native handle finalizer presence is inconsistent",
                        prefix()
                    ));
                }
                if handle.has_finalizer && handle.ownership != "owned" {
                    errors.push(format!(
                        "{} native handle finalizer requires owned ownership",
                        prefix()
                    ));
                }
                if handle.has_finalizer && abi.direction == NativeAbiDirection::Param {
                    errors.push(format!(
                        "{} native handle param must not carry a finalizer",
                        prefix()
                    ));
                }
            }
            None => errors.push(format!(
                "{} handle ABI missing native_handle contract",
                prefix()
            )),
        }
    } else if abi.native_handle.is_some() {
        errors.push(format!(
            "{} non-handle ABI must not carry native_handle contract",
            prefix()
        ));
    }
    let rep_matches = match abi.canonical_kind.as_str() {
        "jsvalue" => matches!(&record.native_rep, NativeRep::JsValue),
        "string" | "ptr" | "i64_str" => {
            matches!(
                &record.native_rep,
                NativeRep::StringRef | NativeRep::NativeHandle | NativeRep::JsValue
            )
        }
        "bool" => matches!(&record.native_rep, NativeRep::I1 | NativeRep::I32),
        "i32" => matches!(&record.native_rep, NativeRep::I32),
        "i64" => matches!(&record.native_rep, NativeRep::I64),
        "u32" => matches!(&record.native_rep, NativeRep::U32),
        "u64" => matches!(&record.native_rep, NativeRep::U64),
        "usize" => matches!(&record.native_rep, NativeRep::USize),
        "f32" => matches!(&record.native_rep, NativeRep::F32),
        "f64" => matches!(&record.native_rep, NativeRep::F64 | NativeRep::JsValue),
        "buffer_len" => matches!(&record.native_rep, NativeRep::BufferLen),
        "buffer+len" => matches!(
            &record.native_rep,
            NativeRep::BufferView(_) | NativeRep::USize | NativeRep::BufferLen
        ),
        "pod+count" => matches!(
            &record.native_rep,
            NativeRep::PodRecordView { .. } | NativeRep::USize
        ),
        "handle" => matches!(&record.native_rep, NativeRep::NativeHandle),
        "promise" => matches!(&record.native_rep, NativeRep::PromiseBoundary),
        "pod" => matches!(&record.native_rep, NativeRep::PodRecord { .. }),
        "void" => false,
        _ => false,
    };
    if !rep_matches {
        errors.push(format!(
            "{} native ABI descriptor {} does not match recorded native rep {}",
            prefix(),
            abi.display,
            record.native_rep_name
        ));
    }
}

pub(crate) fn validate_native_abi_runtime_guard(
    record: &NativeRepRecord,
    abi: &crate::native_value::artifact::NativeAbiTypeRecord,
    errors: &mut Vec<String>,
) {
    let prefix = || {
        format!(
            "{}:{} {}",
            record.function, record.block_label, record.consumer
        )
    };
    match abi.direction {
        NativeAbiDirection::Param => match abi.runtime_guard.as_ref() {
            Some(guard) => {
                if guard.helper.is_empty() || guard.requirement.is_empty() {
                    errors.push(format!("{} native ABI runtime guard is empty", prefix()));
                    return;
                }
                if !valid_runtime_guard_helper(abi.canonical_kind.as_str(), &guard.helper) {
                    errors.push(format!(
                        "{} native ABI descriptor {} used wrong runtime guard {}",
                        prefix(),
                        abi.display,
                        guard.helper
                    ));
                }
            }
            None if abi.canonical_kind == "pod"
                && matches!(record.native_rep, NativeRep::PodRecord { .. })
                && record.pod_layout.is_some()
                && record
                    .notes
                    .iter()
                    .any(|note| note == "source=region_local_pod") => {}
            None if abi.canonical_kind == "pod+count"
                && record.pod_record_view.is_some()
                && record
                    .notes
                    .iter()
                    .any(|note| note == "source=local_pod_view") => {}
            None if abi.canonical_kind != "jsvalue" => {
                errors.push(format!(
                    "{} native ABI param {} missing runtime guard",
                    prefix(),
                    abi.display
                ));
            }
            None => {}
        },
        NativeAbiDirection::Return => {
            if abi.runtime_guard.is_some() {
                errors.push(format!(
                    "{} native ABI return must not carry a runtime guard",
                    prefix()
                ));
            }
        }
    }
}

pub(crate) fn valid_runtime_guard_helper(kind: &str, helper: &str) -> bool {
    match kind {
        "jsvalue" => false,
        "string" => helper == "js_native_abi_check_string_ptr",
        "json" => helper == "js_json_stringify",
        "bool" => helper == "js_is_truthy",
        "i32" => helper == "js_native_abi_check_i32",
        "i64" | "i64_str" => helper == "js_native_abi_check_i64",
        "u32" | "buffer_len" => helper == "js_native_abi_check_u32",
        "u64" => helper == "js_native_abi_check_u64",
        "usize" => helper == "js_native_abi_check_usize",
        "f32" => helper == "js_native_abi_check_f32",
        "f64" => helper == "js_native_abi_check_f64",
        "ptr" => helper == "js_native_abi_check_ptr",
        "buffer+len" => {
            matches!(
                helper,
                "js_native_abi_check_buffer_data_ptr" | "js_native_abi_check_buffer_byte_len"
            )
        }
        "pod+count" => {
            matches!(
                helper,
                "js_native_abi_check_pod_view_data_ptr"
                    | "js_native_abi_check_pod_view_record_count"
            )
        }
        "handle" => helper == "js_native_handle_unwrap",
        "promise" => helper == "js_native_abi_check_promise",
        "pod" => helper == "js_native_abi_check_pod_object",
        "void" => false,
        _ => false,
    }
}

pub(crate) fn validate_buffer_span_pairs(records: &[NativeRepRecord], errors: &mut Vec<String>) {
    for (idx, record) in records.iter().enumerate() {
        let Some(abi) = record.native_abi_type.as_ref() else {
            continue;
        };
        if abi.direction != NativeAbiDirection::Param || abi.canonical_kind != "buffer+len" {
            continue;
        }
        let Some(js_arg) = abi.js_argument_index else {
            continue;
        };
        let Some(guard) = abi.runtime_guard.as_ref() else {
            continue;
        };
        let prefix = || {
            format!(
                "{}:{} {}",
                record.function, record.block_label, record.consumer
            )
        };
        let partner_helper = match guard.helper.as_str() {
            "js_native_abi_check_buffer_data_ptr" => "js_native_abi_check_buffer_byte_len",
            "js_native_abi_check_buffer_byte_len" => "js_native_abi_check_buffer_data_ptr",
            _ => continue,
        };
        let expected_partner_slot = if guard.helper == "js_native_abi_check_buffer_data_ptr" {
            abi.abi_slot_index + 1
        } else if abi.abi_slot_index == 0 {
            errors.push(format!(
                "{} buffer+len byte_len slot has no preceding data slot",
                prefix()
            ));
            continue;
        } else {
            abi.abi_slot_index - 1
        };
        let found_partner = records.iter().enumerate().any(|(other_idx, other)| {
            if other_idx == idx {
                return false;
            }
            let Some(other_abi) = other.native_abi_type.as_ref() else {
                return false;
            };
            other.function == record.function
                && other.block_label == record.block_label
                && other_abi.direction == NativeAbiDirection::Param
                && other_abi.canonical_kind == "buffer+len"
                && other_abi.js_argument_index == Some(js_arg)
                && other_abi.abi_slot_index == expected_partner_slot
                && other_abi.abi_slot_count == 2
                && other_abi
                    .runtime_guard
                    .as_ref()
                    .is_some_and(|other_guard| other_guard.helper == partner_helper)
        });
        if !found_partner {
            errors.push(format!(
                "{} buffer+len ABI slot is not paired with its buffer span partner",
                prefix()
            ));
        }
    }
}

pub(crate) fn validate_pod_view_span_pairs(records: &[NativeRepRecord], errors: &mut Vec<String>) {
    for (idx, record) in records.iter().enumerate() {
        let Some(abi) = record.native_abi_type.as_ref() else {
            continue;
        };
        if abi.direction != NativeAbiDirection::Param || abi.canonical_kind != "pod+count" {
            continue;
        }
        let Some(js_arg) = abi.js_argument_index else {
            continue;
        };
        let Some(guard) = abi.runtime_guard.as_ref() else {
            continue;
        };
        let prefix = || {
            format!(
                "{}:{} {}",
                record.function, record.block_label, record.consumer
            )
        };
        let partner_helper = match guard.helper.as_str() {
            "js_native_abi_check_pod_view_data_ptr" => "js_native_abi_check_pod_view_record_count",
            "js_native_abi_check_pod_view_record_count" => "js_native_abi_check_pod_view_data_ptr",
            _ => continue,
        };
        let expected_partner_slot = if guard.helper == "js_native_abi_check_pod_view_data_ptr" {
            abi.abi_slot_index + 1
        } else if abi.abi_slot_index == 0 {
            errors.push(format!(
                "{} pod+count record_count slot has no preceding data slot",
                prefix()
            ));
            continue;
        } else {
            abi.abi_slot_index - 1
        };
        let found_partner = records.iter().enumerate().any(|(other_idx, other)| {
            if other_idx == idx {
                return false;
            }
            let Some(other_abi) = other.native_abi_type.as_ref() else {
                return false;
            };
            other.function == record.function
                && other.block_label == record.block_label
                && other_abi.direction == NativeAbiDirection::Param
                && other_abi.canonical_kind == "pod+count"
                && other_abi.js_argument_index == Some(js_arg)
                && other_abi.abi_slot_index == expected_partner_slot
                && other_abi.abi_slot_count == 2
                && other_abi
                    .runtime_guard
                    .as_ref()
                    .is_some_and(|other_guard| other_guard.helper == partner_helper)
        });
        if !found_partner {
            errors.push(format!(
                "{} pod+count ABI slot is not paired with its record-view partner",
                prefix()
            ));
        }
    }
}
