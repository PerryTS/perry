//! Project-asset copy helpers used by the per-target bundle writers.
//!
//! Extracted from `compile.rs` for issue #1105 PR 3 (directory split).
//! Pure file move — no behavior change. Three callers from the parent
//! orchestrator and the various `bundle_for_*` helpers all share these
//! routines for locating the project root and copying its asset
//! directories into the platform bundle.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context, Result};

use crate::OutputFormat;

use super::{CompilationContext, NativeBackend, NativeBackendConfig};

const BACKEND_PACKAGE_METADATA_FILE: &str = "perry-backend-package.json";

/// Walk up from `start` looking for a project anchor (`package.json`,
/// or `perry.toml` if `watch_for_perry_toml` is `true`). Bounded to 5
/// levels so a runaway walk can't traverse the filesystem. Returns
/// the deepest directory that holds an anchor; if none found within
/// the bound, returns the starting input unchanged.
pub(super) fn find_project_root_for_resources(start: &Path, watch_for_perry_toml: bool) -> PathBuf {
    let mut project_root = start.to_path_buf();
    for _ in 0..5 {
        if project_root.join("package.json").exists() {
            break;
        }
        if watch_for_perry_toml && project_root.join("perry.toml").exists() {
            break;
        }
        if let Some(parent) = project_root.parent() {
            project_root = parent.to_path_buf();
        } else {
            break;
        }
    }
    project_root
}

/// Recursive copy used by the per-target bundle writers. Mirrors the
/// inline `copy_dir_recursive_standalone` / `copy_dir_recursive`
/// helpers that lived in each bundle branch before PR 2.
pub(super) fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dest_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &dest_path)?;
        } else {
            fs::copy(entry.path(), &dest_path)?;
        }
    }
    Ok(())
}

/// Copy the `logo` / `assets` / `resources` / `images` directories
/// from `project_root` into `dest_dir`. Used by the bundle writers
/// so `[[NSBundle mainBundle] resourcePath]` / `resolve_asset_path`
/// can find assets at runtime.
pub(super) fn copy_bundle_resource_dirs(project_root: &Path, dest_dir: &Path) {
    for dir_name in &["logo", "assets", "resources", "images"] {
        let resource_dir = project_root.join(dir_name);
        if resource_dir.is_dir() {
            let dest = dest_dir.join(dir_name);
            let _ = copy_dir_recursive(&resource_dir, &dest);
        }
    }
}

/// Copy target/backend-owned native-library resources into a stable
/// bundle subdirectory. Perry does not interpret these files; it only
/// preserves the package boundary so native code can resolve its own
/// backend artifacts without app-specific APIs.
pub(super) fn copy_native_library_resources(
    ctx: &CompilationContext,
    dest_dir: &Path,
) -> Result<()> {
    for native_lib in &ctx.native_libraries {
        let Some(target_config) = native_lib.target_config.as_ref() else {
            continue;
        };
        if !target_config.available {
            continue;
        }

        let module_dir = dest_dir
            .join("NativeLibraries")
            .join(sanitize_resource_component(&native_lib.module));
        for path in target_config
            .resources
            .iter()
            .chain(target_config.shader_outputs.iter())
        {
            copy_resource_entry(path, &module_dir).with_context(|| {
                format!(
                    "copying native resource declared by {}: {}",
                    native_lib.module,
                    path.display()
                )
            })?;
        }

        for backend in &target_config.backends {
            if !backend.available {
                continue;
            }
            let backend_dir = module_dir.join(backend_component(backend.backend));
            for path in backend
                .resources
                .iter()
                .chain(backend.shader_outputs.iter())
            {
                copy_resource_entry(path, &backend_dir).with_context(|| {
                    format!(
                        "copying {} backend resource declared by {}: {}",
                        backend.backend.as_str(),
                        native_lib.module,
                        path.display()
                    )
                })?;
            }
            write_backend_package_metadata(&backend_dir, backend)?;
        }
    }
    Ok(())
}

/// Compile backend shader sources that need platform SDK tools and
/// then copy any declared native-library resource artifacts. Metal
/// shaders stay in `compile_metallib_for_bundle` because Apple bundles
/// combine them into one `default.metallib`; Vulkan/D3D12 sources keep
/// their package/backend boundaries under `NativeLibraries/`.
pub(super) fn stage_native_library_artifacts(
    ctx: &CompilationContext,
    dest_dir: &Path,
    format: OutputFormat,
) -> Result<()> {
    compile_backend_shader_sources(ctx, dest_dir, format)?;
    copy_native_library_resources(ctx, dest_dir)
}

pub(super) fn compile_backend_shader_sources(
    ctx: &CompilationContext,
    dest_dir: &Path,
    format: OutputFormat,
) -> Result<()> {
    for native_lib in &ctx.native_libraries {
        let Some(target_config) = native_lib.target_config.as_ref() else {
            continue;
        };
        if !target_config.available {
            continue;
        }

        let module_dir = dest_dir
            .join("NativeLibraries")
            .join(sanitize_resource_component(&native_lib.module));
        for backend in &target_config.backends {
            if !backend.available || backend.shader_sources.is_empty() {
                continue;
            }
            if backend.backend == NativeBackend::Metal {
                continue;
            }

            let backend_dir = module_dir.join(backend_component(backend.backend));
            fs::create_dir_all(&backend_dir)?;
            write_backend_package_metadata(&backend_dir, backend)?;
            for src in &backend.shader_sources {
                let output = compiled_shader_output_path(&backend_dir, backend.backend, src)?;
                compile_backend_shader_source(backend.backend, src, &output).with_context(
                    || {
                        format!(
                            "compiling {} shader declared by {}: {}",
                            backend.backend.as_str(),
                            native_lib.module,
                            src.display()
                        )
                    },
                )?;
                if matches!(format, OutputFormat::Text) {
                    println!(
                        "Compiled {} shader: {} -> {}",
                        backend.backend.as_str(),
                        src.display(),
                        output.display()
                    );
                }
            }
        }
    }
    Ok(())
}

fn copy_resource_entry(src: &Path, dst_dir: &Path) -> std::io::Result<()> {
    let name = src.file_name().unwrap_or_default();
    let dst = dst_dir.join(name);
    if src.is_dir() {
        copy_dir_recursive(src, &dst)
    } else {
        fs::create_dir_all(dst_dir)?;
        fs::copy(src, dst)?;
        Ok(())
    }
}

fn write_backend_package_metadata(dst_dir: &Path, backend: &NativeBackendConfig) -> Result<()> {
    if backend.package.name.is_none()
        && backend.package.version.is_none()
        && backend.package.kind.is_none()
    {
        return Ok(());
    }

    fs::create_dir_all(dst_dir)?;
    let mut metadata = serde_json::Map::new();
    metadata.insert(
        "backend".to_string(),
        serde_json::Value::String(backend.backend.as_str().to_string()),
    );
    if let Some(name) = backend.package.name.as_ref() {
        metadata.insert(
            "name".to_string(),
            serde_json::Value::String(name.to_string()),
        );
    }
    if let Some(version) = backend.package.version.as_ref() {
        metadata.insert(
            "version".to_string(),
            serde_json::Value::String(version.to_string()),
        );
    }
    if let Some(kind) = backend.package.kind.as_ref() {
        metadata.insert(
            "kind".to_string(),
            serde_json::Value::String(kind.to_string()),
        );
    }
    let bytes = serde_json::to_vec_pretty(&serde_json::Value::Object(metadata))?;
    fs::write(dst_dir.join(BACKEND_PACKAGE_METADATA_FILE), bytes)?;
    Ok(())
}

fn compiled_shader_output_path(
    dst_dir: &Path,
    backend: NativeBackend,
    src: &Path,
) -> Result<PathBuf> {
    let file_name = src
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            anyhow!(
                "native {} shader source has no file name: {}",
                backend.as_str(),
                src.display()
            )
        })?;
    Ok(dst_dir.join(format!(
        "{file_name}.{}",
        compiled_shader_extension(backend)
    )))
}

fn compiled_shader_extension(backend: NativeBackend) -> &'static str {
    match backend {
        NativeBackend::Metal => "metallib",
        NativeBackend::Vulkan => "spv",
        NativeBackend::D3d12 => "dxil",
    }
}

fn compile_backend_shader_source(backend: NativeBackend, src: &Path, output: &Path) -> Result<()> {
    let tool = shader_tool_path(backend);
    let mut cmd = Command::new(&tool);
    match backend {
        NativeBackend::Metal => unreachable!("Metal shaders are compiled by xcrun metal"),
        NativeBackend::Vulkan => {
            cmd.arg(src).arg("-o").arg(output);
        }
        NativeBackend::D3d12 => {
            cmd.arg(src).arg("-Fo").arg(output);
        }
    }

    let status = cmd.status().map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            anyhow!(
                "could not find {} for {} shader source {}. Install the SDK toolchain or set {}",
                shader_tool_name(backend),
                backend.as_str(),
                src.display(),
                shader_tool_env_var(backend)
            )
        } else {
            anyhow!(
                "failed to run {} for {} shader source {}: {}",
                tool.display(),
                backend.as_str(),
                src.display(),
                err
            )
        }
    })?;
    if !status.success() {
        return Err(anyhow!(
            "{} failed for {} shader source {}",
            tool.display(),
            backend.as_str(),
            src.display()
        ));
    }
    Ok(())
}

fn shader_tool_path(backend: NativeBackend) -> PathBuf {
    std::env::var_os(shader_tool_env_var(backend))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(shader_tool_name(backend)))
}

fn shader_tool_name(backend: NativeBackend) -> &'static str {
    match backend {
        NativeBackend::Metal => "xcrun",
        NativeBackend::Vulkan => "glslc",
        NativeBackend::D3d12 => "dxc",
    }
}

fn shader_tool_env_var(backend: NativeBackend) -> &'static str {
    match backend {
        NativeBackend::Metal => "PERRY_XCRUN",
        NativeBackend::Vulkan => "PERRY_GLSLC",
        NativeBackend::D3d12 => "PERRY_DXC",
    }
}

fn backend_component(backend: NativeBackend) -> &'static str {
    match backend {
        NativeBackend::Metal => "metal",
        NativeBackend::Vulkan => "vulkan",
        NativeBackend::D3d12 => "d3d12",
    }
}

fn sanitize_resource_component(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "native-library".to_string()
    } else {
        out
    }
}

#[cfg(test)]
mod native_resource_tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};

    use crate::OutputFormat;

    use super::super::{
        CompilationContext, NativeBackend, NativeBackendConfig, NativeBackendPackageMetadata,
        NativeLibraryManifest, TargetNativeConfig,
    };
    use super::{
        copy_native_library_resources, stage_native_library_artifacts,
        BACKEND_PACKAGE_METADATA_FILE,
    };

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    #[test]
    fn copies_target_and_backend_resources_into_package_boundaries() {
        let dir = tempfile::tempdir().unwrap();
        let package_dir = dir.path().join("node_modules/@scope/demo");
        let bundle_dir = dir.path().join("Demo.app");
        fs::create_dir_all(&package_dir).unwrap();
        fs::create_dir_all(&bundle_dir).unwrap();

        let target_resource = package_dir.join("target-config.json");
        let target_shader_dir = package_dir.join("target-shaders");
        let target_shader = target_shader_dir.join("default.metallib");
        let backend_resource = package_dir.join("vk-config.json");
        let backend_shader = package_dir.join("default.spv");
        fs::create_dir_all(&target_shader_dir).unwrap();
        fs::write(&target_resource, b"{\"target\":true}").unwrap();
        fs::write(&target_shader, b"metallib").unwrap();
        fs::write(&backend_resource, b"{\"backend\":\"vulkan\"}").unwrap();
        fs::write(&backend_shader, b"spv").unwrap();

        let mut ctx = CompilationContext::new(dir.path().to_path_buf());
        ctx.native_libraries.push(NativeLibraryManifest {
            module: "@scope/demo".to_string(),
            package_dir: package_dir.clone(),
            abi_version: None,
            functions: Vec::new(),
            target_config: Some(TargetNativeConfig {
                available: true,
                unavailable_reason: None,
                crate_path: PathBuf::new(),
                lib_name: "demo".to_string(),
                prebuilt: None,
                frameworks: Vec::new(),
                optional_frameworks: Vec::new(),
                frameworks_env: None,
                libs: Vec::new(),
                lib_dirs: Vec::new(),
                pkg_config: Vec::new(),
                resources: vec![target_resource],
                shader_outputs: vec![target_shader_dir],
                backends: vec![
                    NativeBackendConfig {
                        backend: NativeBackend::Vulkan,
                        available: true,
                        unavailable_reason: None,
                        prebuilt: None,
                        frameworks: Vec::new(),
                        libs: Vec::new(),
                        lib_dirs: Vec::new(),
                        pkg_config: Vec::new(),
                        shader_sources: Vec::new(),
                        shader_outputs: vec![backend_shader],
                        resources: vec![backend_resource],
                        package: NativeBackendPackageMetadata {
                            name: Some("demo-vulkan".to_string()),
                            version: Some("1.2.3".to_string()),
                            kind: Some("spirv".to_string()),
                        },
                    },
                    NativeBackendConfig {
                        backend: NativeBackend::D3d12,
                        available: false,
                        unavailable_reason: Some("windows only".to_string()),
                        prebuilt: None,
                        frameworks: Vec::new(),
                        libs: Vec::new(),
                        lib_dirs: Vec::new(),
                        pkg_config: Vec::new(),
                        shader_sources: Vec::new(),
                        shader_outputs: Vec::new(),
                        resources: vec![package_dir.join("should-not-copy.bin")],
                        package: NativeBackendPackageMetadata::default(),
                    },
                ],
                swift_sources: Vec::new(),
                metal_sources: Vec::new(),
            }),
        });

        copy_native_library_resources(&ctx, &bundle_dir).expect("copy native resources");

        let package_resource_dir = bundle_dir.join("NativeLibraries/_scope_demo");
        assert!(package_resource_dir.join("target-config.json").is_file());
        assert!(package_resource_dir
            .join("target-shaders/default.metallib")
            .is_file());
        assert!(package_resource_dir.join("vulkan/vk-config.json").is_file());
        assert!(package_resource_dir.join("vulkan/default.spv").is_file());
        let metadata = fs::read_to_string(
            package_resource_dir
                .join("vulkan")
                .join(BACKEND_PACKAGE_METADATA_FILE),
        )
        .expect("read backend package metadata");
        let metadata: serde_json::Value =
            serde_json::from_str(&metadata).expect("parse backend package metadata");
        assert_eq!(metadata["backend"], "vulkan");
        assert_eq!(metadata["name"], "demo-vulkan");
        assert_eq!(metadata["version"], "1.2.3");
        assert_eq!(metadata["kind"], "spirv");
        assert!(!package_resource_dir.join("d3d12").exists());
    }

    #[cfg(unix)]
    #[test]
    fn stages_vulkan_and_d3d12_shader_sources_with_fake_tools() {
        use std::os::unix::fs::PermissionsExt;

        let _guard = ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("env lock");
        let dir = tempfile::tempdir().unwrap();
        let package_dir = dir.path().join("node_modules/@scope/demo");
        let bundle_dir = dir.path().join("out");
        fs::create_dir_all(&package_dir).unwrap();
        fs::create_dir_all(&bundle_dir).unwrap();

        let fake_glslc = dir.path().join("fake-glslc");
        fs::write(
            &fake_glslc,
            "#!/bin/sh\nout=\"\"\nprev=\"\"\nfor arg in \"$@\"; do\n  if [ \"$prev\" = \"-o\" ]; then out=\"$arg\"; fi\n  prev=\"$arg\"\ndone\nif [ -z \"$out\" ]; then exit 2; fi\nmkdir -p \"$(dirname \"$out\")\"\nprintf spv > \"$out\"\nexit 0\n",
        )
        .unwrap();
        let fake_dxc = dir.path().join("fake-dxc");
        fs::write(
            &fake_dxc,
            "#!/bin/sh\nout=\"\"\nprev=\"\"\nfor arg in \"$@\"; do\n  if [ \"$prev\" = \"-Fo\" ]; then out=\"$arg\"; fi\n  prev=\"$arg\"\ndone\nif [ -z \"$out\" ]; then exit 2; fi\nmkdir -p \"$(dirname \"$out\")\"\nprintf dxil > \"$out\"\nexit 0\n",
        )
        .unwrap();
        for tool in [&fake_glslc, &fake_dxc] {
            let mut perms = fs::metadata(tool).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(tool, perms).unwrap();
        }

        let vulkan_shader = package_dir.join("compute.glsl");
        let d3d12_shader = package_dir.join("compute.hlsl");
        fs::write(&vulkan_shader, "#version 450\nvoid main() {}\n").unwrap();
        fs::write(&d3d12_shader, "[numthreads(1,1,1)] void main() {}\n").unwrap();

        let mut ctx = CompilationContext::new(dir.path().to_path_buf());
        ctx.native_libraries.push(NativeLibraryManifest {
            module: "@scope/demo".to_string(),
            package_dir: package_dir.clone(),
            abi_version: None,
            functions: Vec::new(),
            target_config: Some(TargetNativeConfig {
                available: true,
                unavailable_reason: None,
                crate_path: PathBuf::new(),
                lib_name: "demo".to_string(),
                prebuilt: None,
                frameworks: Vec::new(),
                optional_frameworks: Vec::new(),
                frameworks_env: None,
                libs: Vec::new(),
                lib_dirs: Vec::new(),
                pkg_config: Vec::new(),
                resources: Vec::new(),
                shader_outputs: Vec::new(),
                backends: vec![
                    NativeBackendConfig {
                        backend: NativeBackend::Vulkan,
                        available: true,
                        unavailable_reason: None,
                        prebuilt: None,
                        frameworks: Vec::new(),
                        libs: Vec::new(),
                        lib_dirs: Vec::new(),
                        pkg_config: Vec::new(),
                        shader_sources: vec![vulkan_shader],
                        shader_outputs: Vec::new(),
                        resources: Vec::new(),
                        package: NativeBackendPackageMetadata::default(),
                    },
                    NativeBackendConfig {
                        backend: NativeBackend::D3d12,
                        available: true,
                        unavailable_reason: None,
                        prebuilt: None,
                        frameworks: Vec::new(),
                        libs: Vec::new(),
                        lib_dirs: Vec::new(),
                        pkg_config: Vec::new(),
                        shader_sources: vec![d3d12_shader],
                        shader_outputs: Vec::new(),
                        resources: Vec::new(),
                        package: NativeBackendPackageMetadata::default(),
                    },
                ],
                swift_sources: Vec::new(),
                metal_sources: Vec::new(),
            }),
        });

        let old_glslc = std::env::var_os("PERRY_GLSLC");
        let old_dxc = std::env::var_os("PERRY_DXC");
        std::env::set_var("PERRY_GLSLC", &fake_glslc);
        std::env::set_var("PERRY_DXC", &fake_dxc);
        let result = stage_native_library_artifacts(&ctx, &bundle_dir, OutputFormat::Json);
        match old_glslc {
            Some(value) => std::env::set_var("PERRY_GLSLC", value),
            None => std::env::remove_var("PERRY_GLSLC"),
        }
        match old_dxc {
            Some(value) => std::env::set_var("PERRY_DXC", value),
            None => std::env::remove_var("PERRY_DXC"),
        }

        result.expect("stage native shader artifacts");
        let package_resource_dir = bundle_dir.join("NativeLibraries/_scope_demo");
        assert_eq!(
            fs::read(package_resource_dir.join("vulkan/compute.glsl.spv")).unwrap(),
            b"spv"
        );
        assert_eq!(
            fs::read(package_resource_dir.join("d3d12/compute.hlsl.dxil")).unwrap(),
            b"dxil"
        );
    }
}
