use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use perry_api_manifest::{
    NativeAbiType, NativeHandleAbi, NativeHandleOwnership, NativeHandleThreadAffinity,
    NativePodAbi, NativePodFieldAbi, NativePromiseAbi, NativePromiseCompletion,
    NativePromiseThread,
};

use super::super::{
    NativeBackend, NativeBackendConfig, NativeBackendPackageMetadata, NativeFunctionDecl,
    NativeLibraryManifest, TargetNativeConfig,
};

pub(crate) fn validate_native_library_manifest_value(
    package_dir: &Path,
    module_name: &str,
    native_lib: &serde_json::Value,
) -> Result<()> {
    let package_json = package_dir.join("package.json");
    parse_native_library_functions(&package_json, native_lib)?;
    let Some(targets) = native_lib.get("targets") else {
        return Ok(());
    };
    let Some(targets_obj) = targets.as_object() else {
        return Err(anyhow!(
            "native library `{}` has invalid `perry.nativeLibrary.targets`: expected object",
            module_name
        ));
    };
    for (target_key, value) in targets_obj {
        let base_target = base_target_key(target_key).ok_or_else(|| {
            anyhow!(
                "native library `{}` has unsupported target key `perry.nativeLibrary.targets.{}`. \
                 Expected macos, ios, linux, windows, android, web, harmonyos, tvos, watchos, \
                 visionos, or a supported per-arch key such as macos-arm64.",
                module_name,
                target_key
            )
        })?;
        parse_target_native_config(
            package_dir,
            module_name,
            base_target,
            &format!("perry.nativeLibrary.targets.{target_key}"),
            value,
        )?;
    }
    Ok(())
}

pub(super) fn native_manifest_target_key(target: Option<&str>) -> &'static str {
    match target {
        Some("ios-simulator") | Some("ios") => "ios",
        Some("visionos-simulator") | Some("visionos") => "visionos",
        Some("android") => "android",
        Some("tvos-simulator") | Some("tvos") => "tvos",
        Some("watchos-simulator") | Some("watchos") => "watchos",
        Some("harmonyos-simulator") | Some("harmonyos") => "harmonyos",
        // musl resolves the same native-library platform as glibc Linux
        // (#4826). Prebuilt native addons are typically glibc; static-musl
        // apps using them is an inherent limitation, but the platform key
        // stays "linux" so resolution behaves consistently.
        Some("linux")
        | Some("linux-musl")
        | Some("linux-x86_64-musl")
        | Some("linux-aarch64-musl") => "linux",
        // WinUI (#4680) is the same OS/arch as the Win32 target for native
        // library resolution (D3d12/Vulkan backends, x64 prebuilts).
        Some("windows") | Some("windows-winui") => "windows",
        Some("web") => "web",
        Some("macos") => "macos",
        None if cfg!(target_os = "linux") => "linux",
        None if cfg!(target_os = "windows") => "windows",
        _ => "macos",
    }
}

fn base_target_key(target_key: &str) -> Option<&str> {
    const BASES: &[&str] = &[
        "macos",
        "ios",
        "linux",
        "windows",
        "android",
        "web",
        "harmonyos",
        "tvos",
        "watchos",
        "visionos",
    ];
    if BASES.contains(&target_key) {
        return Some(target_key);
    }
    for base in BASES {
        if let Some(suffix) = target_key.strip_prefix(&format!("{base}-")) {
            if matches!(
                suffix,
                "arm64" | "aarch64" | "x64" | "x86_64" | "ia32" | "i686"
            ) {
                return Some(base);
            }
        }
    }
    None
}

/// Check if a package directory has a perry.nativeLibrary field in its package.json
pub(crate) fn has_perry_native_library(package_dir: &Path) -> bool {
    let package_json = package_dir.join("package.json");
    if let Ok(content) = fs::read_to_string(&package_json) {
        if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&content) {
            return pkg
                .get("perry")
                .and_then(|p| p.get("nativeLibrary"))
                .is_some();
        }
    }
    false
}

/// Check if a package directory has `perry.nativeModule: true` in its package.json.
///
/// Packages that set this flag contain Perry-compatible TypeScript source code
/// and should be compiled natively (NativeCompiled) rather than interpreted via V8.
/// This is the mechanism used by `perry-react`, `perry-react-dom`, and similar
/// first-party TypeScript packages that rely on `perry/ui` or other native modules.
pub(crate) fn has_perry_native_module(package_dir: &Path) -> bool {
    let package_json = package_dir.join("package.json");
    if let Ok(content) = fs::read_to_string(&package_json) {
        if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&content) {
            return pkg
                .get("perry")
                .and_then(|p| p.get("nativeModule"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
        }
    }
    false
}

/// Parse a native library manifest from a package's package.json
pub(crate) fn parse_native_library_manifest(
    package_dir: &Path,
    module_name: &str,
    target: Option<&str>,
) -> Result<Option<NativeLibraryManifest>> {
    let package_json = package_dir.join("package.json");
    let content = match fs::read_to_string(&package_json) {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };
    let pkg: serde_json::Value = match serde_json::from_str(&content) {
        Ok(pkg) => pkg,
        Err(_) => return Ok(None),
    };

    let Some(native_lib) = pkg.get("perry").and_then(|p| p.get("nativeLibrary")) else {
        return Ok(None);
    };

    // Issue #466 Phase 2: read the `abiVersion` field that wrappers
    // declare to assert which `perry-ffi` ABI they were built
    // against. Strict enforcement (refuse to load on mismatch)
    // happens in `validate_abi_version` after the manifest is
    // assembled — keeping the parse loose here means we still
    // produce a structured error pointing at the package, instead
    // of silently dropping the manifest.
    let abi_version = native_lib
        .get("abiVersion")
        .and_then(|v| v.as_str())
        .map(String::from);

    let functions = parse_native_library_functions(&package_json, native_lib)?;

    // Parse target config
    let target_key = native_manifest_target_key(target);

    // Issue #860 — prebuilt distribution (esbuild / sharp / swc /
    // lightningcss pattern) needs per-arch target keys so a single
    // package.json can describe `macos-arm64`, `macos-x64`,
    // `linux-x64`, `linux-arm64`, etc. all at once. Probe the
    // `<target>-<arch>` key first; fall back to the bare `<target>`
    // key so the existing on-disk wrappers (which only use
    // `targets.macos`, `targets.linux`, …) keep working unchanged.
    let arch_for_target = arch_for_target_key(target);
    let arch_key = arch_for_target.map(|arch| format!("{}-{}", target_key, arch));

    let targets_block = native_lib.get("targets");
    let target_value = arch_key
        .as_deref()
        .and_then(|k| targets_block.and_then(|t| t.get(k)))
        .or_else(|| targets_block.and_then(|t| t.get(target_key)));

    let target_config = target_value
        .map(|tc| {
            parse_target_native_config(
                package_dir,
                module_name,
                target_key,
                &format!("perry.nativeLibrary.targets.{target_key}"),
                tc,
            )
        })
        .transpose()?;

    Ok(Some(NativeLibraryManifest {
        module: module_name.to_string(),
        package_dir: package_dir.to_path_buf(),
        abi_version,
        functions,
        target_config,
    }))
}

fn parse_native_library_functions(
    package_json: &Path,
    native_lib: &serde_json::Value,
) -> Result<Vec<NativeFunctionDecl>> {
    let functions = native_lib
        .get("functions")
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            anyhow!(
                "{} perry.nativeLibrary.functions must be an array",
                package_json.display()
            )
        })?;

    let mut parsed = Vec::with_capacity(functions.len());
    for (function_index, function) in functions.iter().enumerate() {
        let name = function
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                anyhow!(
                    "{} nativeLibrary.functions[{}] name must be a string",
                    package_json.display(),
                    function_index
                )
            })?
            .to_string();
        let params = function
            .get("params")
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                anyhow!(
                    "{} nativeLibrary.functions[{}] `{}` params must be an array",
                    package_json.display(),
                    function_index,
                    name
                )
            })?;
        let mut parsed_params = Vec::with_capacity(params.len());
        for (param_index, param) in params.iter().enumerate() {
            let descriptor = parse_native_abi_descriptor(
                package_json,
                function_index,
                &name,
                &format!("params[{param_index}]"),
                param,
                NativeAbiDescriptorPosition::Param,
            )?;
            if !descriptor.is_valid_param() {
                return Err(invalid_native_abi_error(
                    package_json,
                    function_index,
                    &name,
                    &format!("params[{param_index}]"),
                    &descriptor.to_string(),
                    "void is only valid as a return descriptor",
                ));
            }
            parsed_params.push(descriptor);
        }
        let returns_value = function.get("returns").ok_or_else(|| {
            anyhow!(
                "{} nativeLibrary.functions[{}] `{}` returns is required",
                package_json.display(),
                function_index,
                name
            )
        })?;
        let returns = parse_native_abi_descriptor(
            package_json,
            function_index,
            &name,
            "returns",
            returns_value,
            NativeAbiDescriptorPosition::Return,
        )?;
        if !returns.is_valid_return() {
            return Err(invalid_native_abi_error(
                package_json,
                function_index,
                &name,
                "returns",
                &returns.to_string(),
                "buffer+len, pod, and pod+count are parameter-only native ABI descriptors",
            ));
        }
        parsed.push(NativeFunctionDecl {
            name,
            params: parsed_params,
            returns,
        });
    }
    Ok(parsed)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NativeAbiDescriptorPosition {
    Param,
    Return,
    Metadata,
}

fn parse_native_abi_descriptor(
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
            NativeAbiDescriptorPosition::Param,
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

fn invalid_native_abi_error(
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

fn parse_target_native_config(
    package_dir: &Path,
    module_name: &str,
    target_key: &str,
    manifest_path: &str,
    tc: &serde_json::Value,
) -> Result<TargetNativeConfig> {
    let Some(obj) = tc.as_object() else {
        return Err(anyhow!(
            "native library `{}` has invalid `{}`: expected object",
            module_name,
            manifest_path
        ));
    };

    let available =
        optional_bool_field(obj, "available", module_name, manifest_path)?.unwrap_or(true);
    let unavailable_reason = optional_alias_string_field(
        obj,
        "unavailableReason",
        "unavailable_reason",
        module_name,
        manifest_path,
    )?;

    if !available {
        return Ok(TargetNativeConfig {
            available,
            unavailable_reason,
            crate_path: PathBuf::new(),
            lib_name: String::new(),
            prebuilt: None,
            frameworks: Vec::new(),
            optional_frameworks: Vec::new(),
            frameworks_env: None,
            libs: Vec::new(),
            lib_dirs: Vec::new(),
            pkg_config: Vec::new(),
            resources: Vec::new(),
            shader_outputs: Vec::new(),
            backends: Vec::new(),
            swift_sources: Vec::new(),
            metal_sources: Vec::new(),
        });
    }

    let prebuilt =
        parse_optional_path_spec(package_dir, obj.get("prebuilt"), module_name, manifest_path)?;
    let backends = parse_backend_configs(package_dir, module_name, target_key, manifest_path, obj)?;

    let has_crate = obj.get("crate").is_some();
    let has_lib = obj.get("lib").is_some();
    if prebuilt.is_none() && (has_crate ^ has_lib) {
        return Err(anyhow!(
            "native library `{}` has incomplete `{}`: `crate` and `lib` must be declared together when `prebuilt` is absent",
            module_name,
            manifest_path
        ));
    }

    Ok(TargetNativeConfig {
        available,
        unavailable_reason,
        crate_path: package_dir.join(
            optional_string_field(obj, "crate", module_name, manifest_path)?.unwrap_or_default(),
        ),
        lib_name: optional_string_field(obj, "lib", module_name, manifest_path)?
            .unwrap_or_default(),
        prebuilt,
        frameworks: parse_optional_string_array(obj, "frameworks", module_name, manifest_path)?,
        // Issue #1304 — vendored-SDK frameworks gated on an env var.
        // Accept both camelCase (`optionalFrameworks`/`frameworksEnv`,
        // matching `libDirs`/`pkgConfig`) and snake_case
        // (`optional_frameworks`/`frameworks_env`, matching
        // `swift_sources`/`metal_sources`) so package authors don't get
        // tripped up by the manifest's mixed casing convention.
        optional_frameworks: parse_optional_alias_string_array(
            obj,
            "optionalFrameworks",
            "optional_frameworks",
            module_name,
            manifest_path,
        )?,
        frameworks_env: optional_alias_string_field(
            obj,
            "frameworksEnv",
            "frameworks_env",
            module_name,
            manifest_path,
        )?,
        libs: parse_optional_string_array(obj, "libs", module_name, manifest_path)?,
        lib_dirs: parse_optional_path_array(
            obj,
            "libDirs",
            package_dir,
            module_name,
            manifest_path,
        )?,
        pkg_config: parse_optional_string_array(obj, "pkgConfig", module_name, manifest_path)?,
        resources: parse_optional_path_array(
            obj,
            "resources",
            package_dir,
            module_name,
            manifest_path,
        )?,
        shader_outputs: parse_optional_alias_path_array(
            obj,
            "shaderOutputs",
            "shader_outputs",
            package_dir,
            module_name,
            manifest_path,
        )?,
        backends,
        swift_sources: parse_optional_path_array(
            obj,
            "swift_sources",
            package_dir,
            module_name,
            manifest_path,
        )?,
        metal_sources: parse_optional_path_array(
            obj,
            "metal_sources",
            package_dir,
            module_name,
            manifest_path,
        )?,
    })
}

fn parse_backend_configs(
    package_dir: &Path,
    module_name: &str,
    target_key: &str,
    manifest_path: &str,
    target_obj: &serde_json::Map<String, serde_json::Value>,
) -> Result<Vec<NativeBackendConfig>> {
    let Some(backends_value) = target_obj.get("backends") else {
        return Ok(Vec::new());
    };
    let Some(backends_obj) = backends_value.as_object() else {
        return Err(anyhow!(
            "native library `{}` has invalid `{}.backends`: expected object",
            module_name,
            manifest_path
        ));
    };

    let mut backends = Vec::with_capacity(backends_obj.len());
    for (name, value) in backends_obj {
        let backend = parse_backend_name(name).ok_or_else(|| {
            anyhow!(
                "native library `{}` has unsupported backend `{}.backends.{}`. \
                 Supported backends are `metal`, `vulkan`, and `d3d12`.",
                module_name,
                manifest_path,
                name
            )
        })?;
        if !backend_supported_on_target(backend, target_key) {
            return Err(anyhow!(
                "native library `{}` declares backend `{}` under `{}` but `{}` is not supported on target `{}`. \
                 Metal is Apple-only, Vulkan is supported on macos/linux/windows/android/harmonyos, and D3D12 is Windows-only.",
                module_name,
                backend.as_str(),
                manifest_path,
                backend.as_str(),
                target_key
            ));
        }
        let backend_path = format!("{manifest_path}.backends.{name}");
        let backend_obj = value.as_object().ok_or_else(|| {
            anyhow!(
                "native library `{}` has invalid `{}`: expected object",
                module_name,
                backend_path
            )
        })?;
        let available = optional_bool_field(backend_obj, "available", module_name, &backend_path)?
            .unwrap_or(true);
        let unavailable_reason = optional_alias_string_field(
            backend_obj,
            "unavailableReason",
            "unavailable_reason",
            module_name,
            &backend_path,
        )?;
        if !available {
            backends.push(NativeBackendConfig {
                backend,
                available,
                unavailable_reason,
                prebuilt: None,
                frameworks: Vec::new(),
                libs: Vec::new(),
                lib_dirs: Vec::new(),
                pkg_config: Vec::new(),
                shader_sources: Vec::new(),
                shader_outputs: Vec::new(),
                resources: Vec::new(),
                package: NativeBackendPackageMetadata::default(),
            });
            continue;
        }

        backends.push(NativeBackendConfig {
            backend,
            available,
            unavailable_reason,
            prebuilt: parse_optional_path_spec(
                package_dir,
                backend_obj.get("prebuilt"),
                module_name,
                &backend_path,
            )?,
            frameworks: parse_optional_string_array(
                backend_obj,
                "frameworks",
                module_name,
                &backend_path,
            )?,
            libs: parse_optional_string_array(backend_obj, "libs", module_name, &backend_path)?,
            lib_dirs: parse_optional_path_array(
                backend_obj,
                "libDirs",
                package_dir,
                module_name,
                &backend_path,
            )?,
            pkg_config: parse_optional_string_array(
                backend_obj,
                "pkgConfig",
                module_name,
                &backend_path,
            )?,
            shader_sources: parse_optional_alias_path_array(
                backend_obj,
                "shaderSources",
                "shader_sources",
                package_dir,
                module_name,
                &backend_path,
            )?,
            shader_outputs: parse_optional_alias_path_array(
                backend_obj,
                "shaderOutputs",
                "shader_outputs",
                package_dir,
                module_name,
                &backend_path,
            )?,
            resources: parse_optional_path_array(
                backend_obj,
                "resources",
                package_dir,
                module_name,
                &backend_path,
            )?,
            package: parse_backend_package_metadata(backend_obj, module_name, &backend_path)?,
        });
    }
    Ok(backends)
}

fn parse_backend_package_metadata(
    backend_obj: &serde_json::Map<String, serde_json::Value>,
    module_name: &str,
    backend_path: &str,
) -> Result<NativeBackendPackageMetadata> {
    let Some(package) = backend_obj.get("package") else {
        return Ok(NativeBackendPackageMetadata::default());
    };
    let Some(package_obj) = package.as_object() else {
        return Err(anyhow!(
            "native library `{}` has invalid `{}.package`: expected object",
            module_name,
            backend_path
        ));
    };
    Ok(NativeBackendPackageMetadata {
        name: optional_string_field(
            package_obj,
            "name",
            module_name,
            &format!("{backend_path}.package"),
        )?,
        version: optional_string_field(
            package_obj,
            "version",
            module_name,
            &format!("{backend_path}.package"),
        )?,
        kind: optional_string_field(
            package_obj,
            "kind",
            module_name,
            &format!("{backend_path}.package"),
        )?,
    })
}

fn parse_backend_name(name: &str) -> Option<NativeBackend> {
    match name {
        "metal" => Some(NativeBackend::Metal),
        "vulkan" => Some(NativeBackend::Vulkan),
        "d3d12" => Some(NativeBackend::D3d12),
        _ => None,
    }
}

fn backend_supported_on_target(backend: NativeBackend, target_key: &str) -> bool {
    match backend {
        NativeBackend::Metal => matches!(
            target_key,
            "macos" | "ios" | "tvos" | "watchos" | "visionos"
        ),
        NativeBackend::Vulkan => matches!(
            target_key,
            "macos" | "linux" | "windows" | "android" | "harmonyos"
        ),
        NativeBackend::D3d12 => target_key == "windows",
    }
}

fn optional_string_field(
    obj: &serde_json::Map<String, serde_json::Value>,
    field: &str,
    module_name: &str,
    manifest_path: &str,
) -> Result<Option<String>> {
    let Some(value) = obj.get(field) else {
        return Ok(None);
    };
    value.as_str().map(|s| Some(s.to_string())).ok_or_else(|| {
        anyhow!(
            "native library `{}` has invalid `{}.{}`: expected string",
            module_name,
            manifest_path,
            field
        )
    })
}

fn optional_alias_string_field(
    obj: &serde_json::Map<String, serde_json::Value>,
    camel: &str,
    snake: &str,
    module_name: &str,
    manifest_path: &str,
) -> Result<Option<String>> {
    if obj.contains_key(camel) {
        optional_string_field(obj, camel, module_name, manifest_path)
    } else {
        optional_string_field(obj, snake, module_name, manifest_path)
    }
}

fn optional_bool_field(
    obj: &serde_json::Map<String, serde_json::Value>,
    field: &str,
    module_name: &str,
    manifest_path: &str,
) -> Result<Option<bool>> {
    let Some(value) = obj.get(field) else {
        return Ok(None);
    };
    value.as_bool().map(Some).ok_or_else(|| {
        anyhow!(
            "native library `{}` has invalid `{}.{}`: expected boolean",
            module_name,
            manifest_path,
            field
        )
    })
}

fn parse_optional_string_array(
    obj: &serde_json::Map<String, serde_json::Value>,
    field: &str,
    module_name: &str,
    manifest_path: &str,
) -> Result<Vec<String>> {
    let Some(value) = obj.get(field) else {
        return Ok(Vec::new());
    };
    parse_string_array_value(value, module_name, &format!("{manifest_path}.{field}"))
}

fn parse_optional_alias_string_array(
    obj: &serde_json::Map<String, serde_json::Value>,
    camel: &str,
    snake: &str,
    module_name: &str,
    manifest_path: &str,
) -> Result<Vec<String>> {
    if obj.contains_key(camel) {
        parse_optional_string_array(obj, camel, module_name, manifest_path)
    } else {
        parse_optional_string_array(obj, snake, module_name, manifest_path)
    }
}

fn parse_string_array_value(
    value: &serde_json::Value,
    module_name: &str,
    path: &str,
) -> Result<Vec<String>> {
    let Some(arr) = value.as_array() else {
        return Err(anyhow!(
            "native library `{}` has invalid `{}`: expected array of strings",
            module_name,
            path
        ));
    };
    let mut out = Vec::with_capacity(arr.len());
    for (idx, item) in arr.iter().enumerate() {
        let Some(s) = item.as_str() else {
            return Err(anyhow!(
                "native library `{}` has invalid `{}[{}]`: expected string",
                module_name,
                path,
                idx
            ));
        };
        out.push(s.to_string());
    }
    Ok(out)
}

fn parse_optional_path_array(
    obj: &serde_json::Map<String, serde_json::Value>,
    field: &str,
    package_dir: &Path,
    module_name: &str,
    manifest_path: &str,
) -> Result<Vec<PathBuf>> {
    let values = parse_optional_string_array(obj, field, module_name, manifest_path)?;
    Ok(values.into_iter().map(|p| package_dir.join(p)).collect())
}

fn parse_optional_alias_path_array(
    obj: &serde_json::Map<String, serde_json::Value>,
    camel: &str,
    snake: &str,
    package_dir: &Path,
    module_name: &str,
    manifest_path: &str,
) -> Result<Vec<PathBuf>> {
    if obj.contains_key(camel) {
        parse_optional_path_array(obj, camel, package_dir, module_name, manifest_path)
    } else {
        parse_optional_path_array(obj, snake, package_dir, module_name, manifest_path)
    }
}

fn parse_optional_path_spec(
    package_dir: &Path,
    value: Option<&serde_json::Value>,
    module_name: &str,
    manifest_path: &str,
) -> Result<Option<PathBuf>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let spec = value.as_str().ok_or_else(|| {
        anyhow!(
            "native library `{}` has invalid `{}.prebuilt`: expected string",
            module_name,
            manifest_path
        )
    })?;
    resolve_prebuilt_path(package_dir, spec)
        .map(Some)
        .ok_or_else(|| {
            anyhow!(
                "native library `{}` could not resolve `{}.prebuilt` value `{}`. \
                 Use a relative path, an absolute path, or an installed node_modules subpath.",
                module_name,
                manifest_path,
                spec
            )
        })
}

/// Map a Perry target string to the architecture token used in
/// per-arch manifest keys (e.g. `targets.macos-arm64`).
///
/// Returns `None` for targets where the architecture is implicit in
/// the target string itself (`ios` is always arm64-on-device, `web`
/// is wasm). The caller falls back to the bare OS-only target key
/// in those cases, so those wrappers don't need to migrate to the
/// per-arch shape introduced by #860.
fn arch_for_target_key(target: Option<&str>) -> Option<&'static str> {
    // Native (no `--target`): use the host arch so a per-arch entry
    // for the current machine wins over the OS-only fallback.
    if target.is_none() {
        return Some(host_arch_token());
    }
    match target {
        // OS-level targets where both arm64 and x64 are real distribution
        // targets — surface the arch so wrappers can ship per-arch
        // prebuilts.
        Some("macos") => Some("arm64"),
        Some("linux") => Some("x64"),
        Some("windows") | Some("windows-winui") => Some("x64"),
        Some("android") => Some("arm64"),
        Some("harmonyos") => Some("arm64"),
        Some("harmonyos-simulator") => Some("x64"),
        // ios/tvos/watchos/visionos: device builds are always arm64 (or
        // arm64_32 for watchOS). Simulators are arm64 on Apple Silicon
        // hosts and x64 on Intel hosts — we don't currently expose the
        // host distinction at the manifest level. Stick with the
        // OS-only key for now; per-arch keys can be added later if
        // wrappers start needing them.
        _ => None,
    }
}

/// Architecture token for the current host, matching what
/// `arch_for_target_key` would return for a native build. Kept in
/// sync with the npm prebuilt-distribution convention used by
/// esbuild/sharp/swc/lightningcss.
fn host_arch_token() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" | "arm64" => "arm64",
        "x86_64" => "x64",
        "x86" => "ia32",
        other => other,
    }
}

/// Resolve a `prebuilt:` manifest entry to an absolute filesystem
/// path. Returns `None` if the entry could not be resolved.
///
/// Accepted shapes (issue #860):
///
/// - `./relative/path.a` or `../relative/path.a` — resolved against
///   the consuming package's directory (`package_dir`).
/// - `/abs/path.a` — used verbatim.
/// - `@scope/pkg/subpath/file.a` or `pkg/subpath/file.a` — resolved
///   as a node-style module reference. We walk up from
///   `package_dir` looking for a `node_modules/<pkg>` that contains
///   `<subpath>/<file.a>`. This matches what `require.resolve` would
///   do for a sibling package installed via npm
///   `optionalDependencies` (the esbuild/sharp pattern).
fn resolve_prebuilt_path(package_dir: &Path, spec: &str) -> Option<PathBuf> {
    if spec.is_empty() {
        return None;
    }

    let path = Path::new(spec);
    if path.is_absolute() {
        return Some(path.to_path_buf());
    }

    if spec.starts_with("./") || spec.starts_with("../") {
        let joined = package_dir.join(spec);
        return Some(joined);
    }

    // Node-style module reference: split off the package name (one
    // segment, or two if it starts with `@scope/`) and treat the
    // remainder as the subpath within that package.
    let (pkg_name, subpath) = split_module_spec(spec)?;

    // Walk up from `package_dir`, probing every `node_modules/<pkg>`
    // until we find a match. The optionalDependency could be installed
    // anywhere along that chain — typically right next to
    // `package_dir`'s parent (sibling under the same `node_modules/`).
    let mut current: Option<&Path> = Some(package_dir);
    while let Some(dir) = current {
        let candidate_pkg = dir.join("node_modules").join(&pkg_name);
        if candidate_pkg.is_dir() {
            let candidate = candidate_pkg.join(&subpath);
            if candidate.exists() {
                return Some(candidate);
            }
        }
        current = dir.parent();
    }

    None
}

/// Split a node-style module reference like
/// `@scope/pkg/lib/foo.a` into `("@scope/pkg", "lib/foo.a")`.
/// Returns `None` if the spec is just a bare package name (no subpath
/// — `prebuilt:` needs to point at a specific file).
pub(super) fn split_module_spec(spec: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = spec.splitn(4, '/').collect();
    if spec.starts_with('@') {
        // Scoped: `@scope/name/<rest>` — package name is first 2
        // segments, subpath is everything after.
        if parts.len() < 3 {
            return None;
        }
        let pkg = format!("{}/{}", parts[0], parts[1]);
        let subpath = parts[2..].join("/");
        Some((pkg, subpath))
    } else {
        // Unscoped: `name/<rest>`.
        if parts.len() < 2 {
            return None;
        }
        let pkg = parts[0].to_string();
        let subpath = parts[1..].join("/");
        Some((pkg, subpath))
    }
}

/// The ABI version the bundled `perry-ffi` ships. External wrappers
/// declare an `abiVersion` semver range that must include this exact
/// version to be allowed to load. Tracked alongside the workspace
/// version — `perry-ffi` ships in lockstep with `perry` itself for
/// the v0.5.x cycle.
pub const PERRY_FFI_ABI_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Validate a wrapper's declared `abiVersion` against the bundled
/// `perry-ffi` version (#466 Phase 2).
///
/// Behavior on this branch (v0.5.x cycle):
/// - `None` (no field) → warning to stderr, compilation continues.
/// - Valid range that includes the bundled version → silent OK.
/// - Valid range that excludes the bundled version → error result.
/// - Unparseable string → error result.
///
/// From v0.6.0 the `None` arm flips to an error too.
pub(crate) fn validate_abi_version(manifest: &NativeLibraryManifest) -> Result<(), String> {
    use semver::{Version, VersionReq};

    let bundled = Version::parse(PERRY_FFI_ABI_VERSION).map_err(|e| {
        format!(
            "internal error: bundled perry-ffi version `{}` is not valid semver: {}",
            PERRY_FFI_ABI_VERSION, e
        )
    })?;

    let Some(declared) = manifest.abi_version.as_deref() else {
        eprintln!(
            "[perry] warning: native library `{}` does not declare \
             `perry.nativeLibrary.abiVersion`. Add it to package.json \
             to assert ABI compatibility — see \
             docs/native-libraries/manifest-v1.md. (v0.5.x cycle: \
             missing field is allowed; from v0.6.0 it will be a hard error.)",
            manifest.module
        );
        return Ok(());
    };

    // Accept bare-major (`"0.5"`) and bare-minor (`"0.5.3"`) by
    // pre-pending a caret if the user didn't supply an operator.
    // Same pragma cargo's manifest parser uses for `^x.y.z` defaults.
    let req_str = if declared
        .chars()
        .next()
        .map(|c| c.is_ascii_digit())
        .unwrap_or(false)
    {
        format!("^{}", declared)
    } else {
        declared.to_string()
    };

    let req = VersionReq::parse(&req_str).map_err(|e| {
        format!(
            "native library `{}` declares an unparseable `abiVersion: \"{}\"`: {}",
            manifest.module, declared, e
        )
    })?;

    if req.matches(&bundled) {
        Ok(())
    } else {
        Err(format!(
            "native library `{}` declares perry-ffi ABI \"{}\" but this Perry \
             build ships perry-ffi {}. Update the package or use a Perry \
             release whose perry-ffi version matches the declared range.",
            manifest.module, declared, PERRY_FFI_ABI_VERSION
        ))
    }
}
