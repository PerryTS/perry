use anyhow::{anyhow, Result};
use perry_api_manifest::{
    NativeAbiType, NativeHandleAbi, NativeHandleOwnership, NativeHandleThreadAffinity,
    NativePodAbi, NativePodFieldAbi, NativePromiseAbi, NativePromiseCompletion,
    NativePromiseThread,
};
use std::collections::HashSet;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NativeAbiDescriptorPosition {
    Param,
    Return,
    Metadata,
}

pub(super) fn parse_native_abi_descriptor(
    package_json: &Path,
    function_index: usize,
    function_name: &str,
    slot: &str,
    value: &serde_json::Value,
    position: NativeAbiDescriptorPosition,
) -> Result<NativeAbiType> {
    if let Some(spelling) = value.as_str() {
        return NativeAbiType::parse_str(spelling).map_err(|err| {
            invalid_native_abi_error(
                package_json,
                function_index,
                function_name,
                slot,
                err.spelling(),
                err.reason(),
            )
        });
    }

    let Some(object) = value.as_object() else {
        return Err(invalid_native_abi_error(
            package_json,
            function_index,
            function_name,
            slot,
            &value.to_string(),
            "descriptor must be a string or object",
        ));
    };
    let kind = object.get("kind").and_then(|v| v.as_str()).ok_or_else(|| {
        invalid_native_abi_error(
            package_json,
            function_index,
            function_name,
            slot,
            &value.to_string(),
            "structured descriptor requires a string `kind` field",
        )
    })?;

    match kind {
        "handle" => {
            let allowed = [
                "kind",
                "type",
                "ownership",
                "nullable",
                "thread",
                "finalizer",
                "debugName",
            ];
            for key in object.keys() {
                if !allowed.contains(&key.as_str()) {
                    return Err(invalid_native_abi_error(
                        package_json,
                        function_index,
                        function_name,
                        slot,
                        &value.to_string(),
                        &format!("unknown handle descriptor field `{key}`"),
                    ));
                }
            }

            let handle_type = match object.get("type") {
                Some(v) => Some(
                    v.as_str()
                        .filter(|s| !s.trim().is_empty())
                        .ok_or_else(|| {
                            invalid_native_abi_error(
                                package_json,
                                function_index,
                                function_name,
                                slot,
                                &value.to_string(),
                                "handle descriptor `type` must be a non-empty string",
                            )
                        })?
                        .to_string(),
                ),
                None => None,
            };
            let ownership = match object.get("ownership") {
                Some(v) => match v.as_str() {
                    Some("borrowed") => NativeHandleOwnership::Borrowed,
                    Some("owned") => NativeHandleOwnership::Owned,
                    Some(_) | None => {
                        return Err(invalid_native_abi_error(
                            package_json,
                            function_index,
                            function_name,
                            slot,
                            &value.to_string(),
                            "handle descriptor `ownership` must be `owned` or `borrowed`",
                        ));
                    }
                },
                None => NativeHandleOwnership::Borrowed,
            };
            let nullable = match object.get("nullable") {
                Some(v) => v.as_bool().ok_or_else(|| {
                    invalid_native_abi_error(
                        package_json,
                        function_index,
                        function_name,
                        slot,
                        &value.to_string(),
                        "handle descriptor `nullable` must be a boolean",
                    )
                })?,
                None => false,
            };
            let thread = match object.get("thread") {
                Some(v) => match v.as_str() {
                    Some("any") => NativeHandleThreadAffinity::Any,
                    Some("main") => NativeHandleThreadAffinity::Main,
                    Some("creator") => NativeHandleThreadAffinity::Creator,
                    Some(_) | None => {
                        return Err(invalid_native_abi_error(
                            package_json,
                            function_index,
                            function_name,
                            slot,
                            &value.to_string(),
                            "handle descriptor `thread` must be `any`, `main`, or `creator`",
                        ));
                    }
                },
                None => NativeHandleThreadAffinity::Any,
            };
            let finalizer = match object.get("finalizer") {
                Some(v) => Some(
                    v.as_str()
                        .filter(|s| !s.trim().is_empty())
                        .ok_or_else(|| {
                            invalid_native_abi_error(
                                package_json,
                                function_index,
                                function_name,
                                slot,
                                &value.to_string(),
                                "handle descriptor `finalizer` must be a non-empty string",
                            )
                        })?
                        .to_string(),
                ),
                None => None,
            };
            if finalizer.is_some() && ownership != NativeHandleOwnership::Owned {
                return Err(invalid_native_abi_error(
                    package_json,
                    function_index,
                    function_name,
                    slot,
                    &value.to_string(),
                    "handle descriptor `finalizer` requires `ownership: \"owned\"`",
                ));
            }
            if finalizer.is_some() && position != NativeAbiDescriptorPosition::Return {
                return Err(invalid_native_abi_error(
                    package_json,
                    function_index,
                    function_name,
                    slot,
                    &value.to_string(),
                    "handle descriptor `finalizer` is valid only on returns",
                ));
            }
            let debug_name = match object.get("debugName") {
                Some(v) => v
                    .as_str()
                    .filter(|s| !s.trim().is_empty())
                    .ok_or_else(|| {
                        invalid_native_abi_error(
                            package_json,
                            function_index,
                            function_name,
                            slot,
                            &value.to_string(),
                            "handle descriptor `debugName` must be a non-empty string",
                        )
                    })?
                    .to_string(),
                None => handle_type.as_deref().unwrap_or("handle").to_string(),
            };

            Ok(NativeAbiType::Handle(NativeHandleAbi {
                type_name: handle_type,
                ownership,
                nullable,
                thread,
                finalizer,
                debug_name,
            }))
        }
        "promise" => {
            let allowed = ["kind", "result", "completion", "thread"];
            for key in object.keys() {
                if !allowed.contains(&key.as_str()) {
                    return Err(invalid_native_abi_error(
                        package_json,
                        function_index,
                        function_name,
                        slot,
                        &value.to_string(),
                        &format!("unknown promise descriptor field `{key}`"),
                    ));
                }
            }
            let result = match object.get("result") {
                Some(result) => parse_native_abi_descriptor(
                    package_json,
                    function_index,
                    function_name,
                    slot,
                    result,
                    NativeAbiDescriptorPosition::Metadata,
                )?,
                None => NativeAbiType::JsValue,
            };
            let completion = match object.get("completion") {
                Some(v) => match v.as_str() {
                    Some("direct") => NativePromiseCompletion::Direct,
                    Some("native_async") => NativePromiseCompletion::NativeAsync,
                    Some(_) | None => {
                        return Err(invalid_native_abi_error(
                            package_json,
                            function_index,
                            function_name,
                            slot,
                            &value.to_string(),
                            "promise descriptor `completion` must be `direct` or `native_async`",
                        ));
                    }
                },
                None => NativePromiseCompletion::Direct,
            };
            if completion == NativePromiseCompletion::NativeAsync
                && position != NativeAbiDescriptorPosition::Return
            {
                return Err(invalid_native_abi_error(
                    package_json,
                    function_index,
                    function_name,
                    slot,
                    &value.to_string(),
                    "native_async promise completion is valid only on returns",
                ));
            }
            let thread = match object.get("thread") {
                Some(v) => match v.as_str() {
                    Some("any") => NativePromiseThread::Any,
                    Some("main") => NativePromiseThread::Main,
                    Some(_) | None => {
                        return Err(invalid_native_abi_error(
                            package_json,
                            function_index,
                            function_name,
                            slot,
                            &value.to_string(),
                            "promise descriptor `thread` must be `any` or `main`",
                        ));
                    }
                },
                None => NativePromiseThread::Any,
            };
            Ok(NativeAbiType::Promise(NativePromiseAbi {
                result: Box::new(result),
                completion,
                thread,
            }))
        }
        "pod" => {
            parse_native_pod_descriptor(package_json, function_index, function_name, slot, value)
        }
        "pod+count" => {
            parse_native_pod_descriptor(package_json, function_index, function_name, slot, value)
                .map(|descriptor| match descriptor {
                    NativeAbiType::Pod(pod) => NativeAbiType::PodAndCount(pod),
                    other => other,
                })
        }
        "buffer+len" => Ok(NativeAbiType::BufferAndLen),
        _ => NativeAbiType::parse_str(kind).map_err(|err| {
            invalid_native_abi_error(
                package_json,
                function_index,
                function_name,
                slot,
                err.spelling(),
                err.reason(),
            )
        }),
    }
}

fn parse_native_pod_descriptor(
    package_json: &Path,
    function_index: usize,
    function_name: &str,
    slot: &str,
    value: &serde_json::Value,
) -> Result<NativeAbiType> {
    let object = value.as_object().expect("pod descriptor is an object");
    let allowed = ["kind", "name", "fields"];
    for key in object.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(invalid_native_abi_error(
                package_json,
                function_index,
                function_name,
                slot,
                &value.to_string(),
                &format!("unknown pod descriptor field `{key}`"),
            ));
        }
    }

    let name = match object.get("name") {
        Some(v) => Some(
            v.as_str()
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| {
                    invalid_native_abi_error(
                        package_json,
                        function_index,
                        function_name,
                        slot,
                        &value.to_string(),
                        "pod descriptor `name` must be a non-empty string",
                    )
                })?
                .to_string(),
        ),
        None => None,
    };

    let fields_value = object.get("fields").ok_or_else(|| {
        invalid_native_abi_error(
            package_json,
            function_index,
            function_name,
            slot,
            &value.to_string(),
            "pod descriptor requires a `fields` array",
        )
    })?;
    let fields_array = fields_value.as_array().ok_or_else(|| {
        invalid_native_abi_error(
            package_json,
            function_index,
            function_name,
            slot,
            &value.to_string(),
            "pod descriptor `fields` must be an array",
        )
    })?;
    if fields_array.is_empty() {
        return Err(invalid_native_abi_error(
            package_json,
            function_index,
            function_name,
            slot,
            &value.to_string(),
            "pod descriptor `fields` must contain at least one field",
        ));
    }

    let mut seen = HashSet::new();
    let mut fields = Vec::with_capacity(fields_array.len());
    for (field_index, field_value) in fields_array.iter().enumerate() {
        let Some(field_object) = field_value.as_object() else {
            return Err(invalid_native_abi_error(
                package_json,
                function_index,
                function_name,
                slot,
                &field_value.to_string(),
                "pod field descriptor must be an object",
            ));
        };
        let allowed = ["name", "type", "abi"];
        for key in field_object.keys() {
            if !allowed.contains(&key.as_str()) {
                return Err(invalid_native_abi_error(
                    package_json,
                    function_index,
                    function_name,
                    slot,
                    &field_value.to_string(),
                    &format!("unknown pod field descriptor field `{key}`"),
                ));
            }
        }
        let field_name = field_object
            .get("name")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| {
                invalid_native_abi_error(
                    package_json,
                    function_index,
                    function_name,
                    &format!("{slot}.fields[{field_index}]"),
                    &field_value.to_string(),
                    "pod field `name` must be a non-empty string",
                )
            })?
            .to_string();
        if !seen.insert(field_name.clone()) {
            return Err(invalid_native_abi_error(
                package_json,
                function_index,
                function_name,
                &format!("{slot}.fields[{field_index}]"),
                &field_value.to_string(),
                &format!("duplicate pod field `{field_name}`"),
            ));
        }
        if field_object.contains_key("type") && field_object.contains_key("abi") {
            return Err(invalid_native_abi_error(
                package_json,
                function_index,
                function_name,
                &format!("{slot}.fields[{field_index}]"),
                &field_value.to_string(),
                "pod field must use only one of `type` or `abi`",
            ));
        }
        let ty_value = field_object
            .get("type")
            .or_else(|| field_object.get("abi"))
            .ok_or_else(|| {
                invalid_native_abi_error(
                    package_json,
                    function_index,
                    function_name,
                    &format!("{slot}.fields[{field_index}]"),
                    &field_value.to_string(),
                    "pod field requires a `type` string",
                )
            })?;
        let ty = parse_native_abi_descriptor(
            package_json,
            function_index,
            function_name,
            &format!("{slot}.fields[{field_index}].type"),
            ty_value,
            NativeAbiDescriptorPosition::Metadata,
        )?;
        if !ty.is_valid_pod_field() {
            return Err(invalid_native_abi_error(
                package_json,
                function_index,
                function_name,
                &format!("{slot}.fields[{field_index}].type"),
                &ty.to_string(),
                "pod field type must be one of i32, i64, u32, u64, usize, f32, f64, number, buffer_len, handle_id, or nested pod",
            ));
        }
        fields.push(NativePodFieldAbi {
            name: field_name,
            ty,
        });
    }

    Ok(NativeAbiType::Pod(NativePodAbi { name, fields }))
}

pub(super) fn invalid_native_abi_error(
    package_json: &Path,
    function_index: usize,
    function_name: &str,
    slot: &str,
    spelling: &str,
    reason: &str,
) -> anyhow::Error {
    anyhow!(
        "{} nativeLibrary.functions[{}] `{}` {} invalid ABI {:?}: {}",
        package_json.display(),
        function_index,
        function_name,
        slot,
        spelling,
        reason
    )
}
