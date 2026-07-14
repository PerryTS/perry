use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use super::{BuildCacheStats, CompilationContext, CompileArgs, CompileResult, LinkCacheStats};

const BUILD_CACHE_MANIFEST_VERSION: u32 = 1;

const BUILD_CACHE_ENV_VARS: &[&str] = &[
    "PATH",
    "LIB",
    "LIBPATH",
    "LIBRARY_PATH",
    "LD_LIBRARY_PATH",
    "DYLD_LIBRARY_PATH",
    "SDKROOT",
    "PKG_CONFIG_PATH",
    "PKG_CONFIG_LIBDIR",
    "PKG_CONFIG_SYSROOT_DIR",
    "PERRY_LINUX_SYSROOT",
    "PERRY_WINDOWS_SYSROOT",
    "PERRY_IOS_SYSROOT",
    "PERRY_MACOS_SYSROOT",
    "PERRY_TVOS_SYSROOT",
    "PERRY_VISIONOS_SYSROOT",
    "ANDROID_NDK_HOME",
    "OHOS_SDK_HOME",
    "HARMONYOS_SDK_HOME",
    "PERRY_DEBUG_INIT",
    "PERRY_DEBUG_SYMBOLS",
    "PERRY_LLVM_CLANG",
    "PERRY_WRITE_BARRIERS",
    "PERRY_SHADOW_STACK",
    "PERRY_DISABLE_BUFFER_FAST_PATH",
    "PERRY_VERIFY_NATIVE_REGIONS",
    "PERRY_UNBOXED_OBJECT_FIELDS",
    // #6125: the resolved CPU baseline (promoted from --march / perry.toml
    // [build] by promote_cpu_baseline_env before this probe runs). Flipping
    // it must invalidate the build-level no-op check, not just per-object
    // cache entries.
    "PERRY_TARGET_CPU",
    "PERRY_NO_AUTO_OPTIMIZE",
    "PERRY_DISABLE_WELL_KNOWN",
    "PERRY_FORCE_WELL_KNOWN",
];

#[derive(Debug, Clone)]
pub(super) struct BuildCacheProbe {
    args_key: String,
    manifest_path: PathBuf,
    output_path: PathBuf,
    target_name: String,
    input_path: PathBuf,
    project_root: PathBuf,
    cache_root: PathBuf,
    eligible: Result<(), String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct BuildCacheManifest {
    version: u32,
    perry_version: String,
    perry_build_id: FileFingerprint,
    args_key: String,
    env: Vec<EnvFingerprint>,
    input_path: String,
    output_path: String,
    target: String,
    compiled_features: Vec<String>,
    sources: Vec<FileFingerprint>,
    config_inputs: Vec<FileFingerprint>,
    runtime_inputs: Vec<FileFingerprint>,
    object_fingerprints: Vec<String>,
    native_modules: usize,
    js_modules: usize,
    output: FileFingerprint,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct FileFingerprint {
    path: String,
    size: u64,
    sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct EnvFingerprint {
    name: String,
    value: Option<String>,
}

impl BuildCacheProbe {
    /// `cache_root` is the config root (where package.json lives) used to
    /// pick up config-file inputs; `cache_dir` is the already-resolved
    /// Perry cache directory the build manifest is written under.
    pub(super) fn new(
        args: &CompileArgs,
        project_root: &Path,
        cache_root: &Path,
        cache_dir: &Path,
    ) -> Self {
        let output_path = default_output_path(args);
        let output_identity = absolute_identity(&output_path);
        let manifest_name = format!("{}.json", short_hash(output_identity.as_bytes()));
        let manifest_path = cache_dir
            .join("build")
            .join(args.target.as_deref().unwrap_or("native"))
            .join(manifest_name);
        let eligible = eligibility(args, project_root);
        Self {
            args_key: args_key(args, &output_path, project_root),
            manifest_path,
            output_path,
            target_name: args.target.clone().unwrap_or_else(|| "native".to_string()),
            input_path: args.input.clone(),
            project_root: project_root.to_path_buf(),
            cache_root: cache_root.to_path_buf(),
            eligible,
        }
    }

    pub(super) fn probe(&self) -> BuildCacheStats {
        if std::env::var("PERRY_DISABLE_BUILD_CACHE").ok().as_deref() == Some("1") {
            return miss("disabled-by-env");
        }
        if let Err(reason) = &self.eligible {
            return miss(reason);
        }
        let raw = match fs::read_to_string(&self.manifest_path) {
            Ok(raw) => raw,
            Err(_) => return miss("manifest-missing"),
        };
        let manifest = match serde_json::from_str::<BuildCacheManifest>(&raw) {
            Ok(manifest) => manifest,
            Err(_) => return miss("manifest-invalid"),
        };
        if manifest.version != BUILD_CACHE_MANIFEST_VERSION {
            return miss("manifest-version");
        }
        if manifest.perry_version != env!("CARGO_PKG_VERSION") {
            return miss("perry-version");
        }
        if manifest.args_key != self.args_key {
            return miss("args");
        }
        if manifest.env != current_env() {
            return miss("env");
        }
        if manifest.input_path != absolute_identity(&self.input_path) {
            return miss("input-path");
        }
        if manifest.output_path != absolute_identity(&self.output_path) {
            return miss("output-path");
        }
        if file_fingerprint_from_str(&manifest.perry_build_id.path).ok()
            != Some(manifest.perry_build_id.clone())
        {
            return miss("perry-build-id");
        }
        if verify_files(&manifest.sources).is_err() {
            return miss("source");
        }
        if verify_files(&manifest.config_inputs).is_err() {
            return miss("config");
        }
        if verify_files(&manifest.runtime_inputs).is_err() {
            return miss("runtime-input");
        }
        if file_fingerprint(&self.output_path).ok() != Some(manifest.output.clone()) {
            return miss("output");
        }
        BuildCacheStats {
            hit: true,
            reason: "manifest-match".to_string(),
        }
    }

    pub(super) fn print_json_hit(&self, stats: &BuildCacheStats) -> Result<()> {
        let manifest = fs::read_to_string(&self.manifest_path)
            .ok()
            .and_then(|raw| serde_json::from_str::<BuildCacheManifest>(&raw).ok());
        let (native_modules, js_modules) = manifest
            .as_ref()
            .map(|m| (m.native_modules, m.js_modules))
            .unwrap_or((0, 0));
        let result = serde_json::json!({
            "success": true,
            "output": self.output_path.to_string_lossy(),
            "native_modules": native_modules,
            "js_modules": js_modules,
            "build_cache": {
                "hit": stats.hit,
                "miss_reason": serde_json::Value::Null,
                "reason": stats.reason,
            },
            "codegen_cache": serde_json::Value::Null,
            "link_cache": {
                "linked": false,
                "skipped": true,
            },
        });
        println!("{}", serde_json::to_string(&result)?);
        Ok(())
    }

    pub(super) fn compile_result_for_hit(&self) -> CompileResult {
        CompileResult {
            output_path: self.output_path.clone(),
            target: self.target_name.clone(),
            bundle_id: None,
            is_dylib: false,
            codegen_cache_stats: None,
            link_cache_stats: Some(LinkCacheStats {
                linked: false,
                skipped: true,
                object_fingerprints_used: 0,
                object_files_hashed: 0,
                external_inputs_hashed: 0,
            }),
            build_cache_stats: Some(BuildCacheStats {
                hit: true,
                reason: "manifest-match".to_string(),
            }),
        }
    }

    pub(super) fn write_manifest_after_success(
        &self,
        stats: &mut BuildCacheStats,
        ctx: &CompilationContext,
        output_path: &Path,
        target: Option<&str>,
        compiled_features: &[String],
        object_fingerprints: &[String],
        runtime_inputs: &[PathBuf],
    ) {
        if std::env::var("PERRY_DISABLE_BUILD_CACHE").ok().as_deref() == Some("1") {
            stats.reason = "disabled-by-env".to_string();
            return;
        }
        if let Err(reason) = &self.eligible {
            stats.reason = reason.clone();
            return;
        }
        if ctx.needs_ui || ctx.needs_geisterhand || ctx.needs_plugins || ctx.needs_wasm_runtime {
            stats.reason = "complex-runtime".to_string();
            return;
        }
        if !ctx.native_libraries.is_empty() {
            stats.reason = "native-libraries".to_string();
            return;
        }
        if !ctx.js_modules.is_empty() {
            stats.reason = "js-modules".to_string();
            return;
        }
        let manifest = match self.build_manifest(
            ctx,
            output_path,
            target,
            compiled_features,
            object_fingerprints,
            runtime_inputs,
        ) {
            Ok(manifest) => manifest,
            Err(reason) => {
                stats.reason = reason;
                return;
            }
        };
        let Some(parent) = self.manifest_path.parent() else {
            stats.reason = "manifest-parent".to_string();
            return;
        };
        if fs::create_dir_all(parent).is_err() {
            stats.reason = "manifest-dir".to_string();
            return;
        }
        let bytes = match serde_json::to_vec_pretty(&manifest) {
            Ok(bytes) => bytes,
            Err(_) => {
                stats.reason = "manifest-serialize".to_string();
                return;
            }
        };
        let tmp = self.manifest_path.with_extension("json.tmp");
        if fs::write(&tmp, bytes).is_ok() && fs::rename(&tmp, &self.manifest_path).is_ok() {
            stats.reason = "stored".to_string();
        } else {
            let _ = fs::remove_file(&tmp);
            stats.reason = "manifest-write".to_string();
        }
    }

    fn build_manifest(
        &self,
        ctx: &CompilationContext,
        output_path: &Path,
        target: Option<&str>,
        compiled_features: &[String],
        object_fingerprints: &[String],
        runtime_inputs: &[PathBuf],
    ) -> Result<BuildCacheManifest, String> {
        let sources = ctx
            .native_modules
            .keys()
            .map(|path| file_fingerprint(path.as_path()))
            .collect::<std::io::Result<Vec<_>>>()
            .map_err(|_| "source-fingerprint".to_string())?;
        let config_inputs = config_inputs_for(&sources, &self.project_root, &self.cache_root)
            .into_iter()
            .map(|path| file_fingerprint(path.as_path()))
            .collect::<std::io::Result<Vec<_>>>()
            .map_err(|_| "config-fingerprint".to_string())?;
        let runtime_inputs = runtime_inputs
            .iter()
            .filter(|p| p.exists())
            .map(|path| file_fingerprint(path.as_path()))
            .collect::<std::io::Result<Vec<_>>>()
            .map_err(|_| "runtime-fingerprint".to_string())?;
        Ok(BuildCacheManifest {
            version: BUILD_CACHE_MANIFEST_VERSION,
            perry_version: env!("CARGO_PKG_VERSION").to_string(),
            perry_build_id: current_perry_fingerprint().map_err(|_| "perry-fingerprint")?,
            args_key: self.args_key.clone(),
            env: current_env(),
            input_path: absolute_identity(&self.input_path),
            output_path: absolute_identity(output_path),
            target: target.unwrap_or("native").to_string(),
            compiled_features: compiled_features.to_vec(),
            sources,
            config_inputs,
            runtime_inputs,
            object_fingerprints: object_fingerprints.to_vec(),
            native_modules: ctx.native_modules.len(),
            js_modules: ctx.js_modules.len(),
            output: file_fingerprint(output_path).map_err(|_| "output-fingerprint".to_string())?,
        })
    }
}

fn miss(reason: &str) -> BuildCacheStats {
    BuildCacheStats {
        hit: false,
        reason: reason.to_string(),
    }
}

fn eligibility(args: &CompileArgs, project_root: &Path) -> Result<(), String> {
    if args.no_cache {
        return Err("no-cache".to_string());
    }
    if args.no_link {
        return Err("no-link".to_string());
    }
    if args.output_type != "executable" {
        return Err("library-output".to_string());
    }
    if matches!(
        args.target.as_deref(),
        Some("web")
            | Some("wasm")
            | Some("ios-widget")
            | Some("ios-widget-simulator")
            | Some("watchos-widget")
            | Some("watchos-widget-simulator")
            | Some("android-widget")
            | Some("wearos-tile")
    ) {
        return Err("non-native-target".to_string());
    }
    if args.bundle_extensions.is_some() {
        return Err("bundle-extensions".to_string());
    }
    if args.enable_wasm_runtime {
        return Err("wasm-runtime".to_string());
    }
    if args.type_check {
        return Err("type-check".to_string());
    }
    if args.print_hir || args.trace.is_some() || args.focus.is_some() {
        return Err("diagnostic-mode".to_string());
    }
    if args.explain_lowering {
        return Err("explain-lowering".to_string());
    }
    if args.verify_native_regions || args.emit_attest || args.emit_sandbox {
        return Err("sidecar-or-verify".to_string());
    }
    if std::env::var("PERRY_NO_CACHE").ok().as_deref() == Some("1") {
        return Err("no-cache-env".to_string());
    }
    if has_resource_copy_side_effects(project_root) {
        return Err("resource-dirs".to_string());
    }
    if package_has_unknown_build_hooks(project_root) {
        return Err("package-codegen".to_string());
    }
    if entry_uses_precompile(&args.input) {
        return Err("precompile".to_string());
    }
    Ok(())
}

fn package_has_unknown_build_hooks(project_root: &Path) -> bool {
    let pkg = project_root.join("package.json");
    if pkg.exists() {
        let Ok(raw) = fs::read_to_string(pkg) else {
            return true;
        };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw) else {
            return true;
        };
        if json.pointer("/perry/codegen").is_some() || json.pointer("/perry/i18n").is_some() {
            return true;
        }
    }

    let toml = project_root.join("perry.toml");
    if toml.exists() {
        let Ok(raw) = fs::read_to_string(toml) else {
            return true;
        };
        if raw.contains("codegen") || raw.contains("i18n") {
            return true;
        }
    }

    false
}

fn has_resource_copy_side_effects(project_root: &Path) -> bool {
    ["logo", "assets", "resources", "images"]
        .into_iter()
        .any(|name| project_root.join(name).exists())
}

fn entry_uses_precompile(input: &Path) -> bool {
    fs::read_to_string(input)
        .map(|src| src.contains("precompile("))
        .unwrap_or(true)
}

fn args_key(args: &CompileArgs, output_path: &Path, project_root: &Path) -> String {
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, "args-debug", &format!("{args:?}"));
    hash_field(&mut hasher, "input", &absolute_identity(&args.input));
    hash_field(&mut hasher, "output", &absolute_identity(output_path));
    hash_field(
        &mut hasher,
        "target",
        args.target.as_deref().unwrap_or("native"),
    );
    hash_field(&mut hasher, "output-type", &args.output_type);
    hash_field(
        &mut hasher,
        "features",
        args.features.as_deref().unwrap_or(""),
    );
    // #5731 — fold the resolved embedded-asset set into the key. `{args:?}`
    // covers `--embed` patterns but not `perry.embed` / `[compile] embed`
    // config nor the files' state, so without this an edit to an embedded file
    // (with no pattern change) would reuse a stale cached binary. Key on each
    // asset's name + size + mtime rather than re-reading and hashing the full
    // contents here — the bytes are already streamed into the binary at embed
    // time, and size+mtime is the conventional, cheap freshness signal (a fresh
    // checkout bumps mtime → safe rebuild; the only miss is a content change
    // that preserves both size and mtime, which real edits don't do).
    //
    // Fail closed on resolution / per-asset stat failures. `run_pipeline`
    // treats `resolve_embedded_assets` as fatal (the `?` at its embed step), so
    // the cache must not let a broken `perry.embed` / `[compile] embed` config —
    // or a file that vanished or can't be stat'd — silently drop the embed
    // inputs and fall back to the non-embed key, which could reuse a stale
    // manifest and mask the error. Folding a sentinel field on every error path
    // makes the key diverge from any successful build (which never emits these
    // field names), so the probe misses, `run_pipeline` re-runs, and the real
    // error surfaces instead of a stale binary.
    match super::embed::resolve_embedded_assets(&args.embed, project_root) {
        Ok(assets) => {
            for (name, path) in &assets {
                hash_field(&mut hasher, "embed-name", name);
                match fs::metadata(path) {
                    Ok(meta) => {
                        hash_field(&mut hasher, "embed-size", &meta.len().to_string());
                        match meta
                            .modified()
                            .ok()
                            .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                        {
                            Some(dur) => hash_field(
                                &mut hasher,
                                "embed-mtime",
                                &format!("{}.{:09}", dur.as_secs(), dur.subsec_nanos()),
                            ),
                            None => hash_field(&mut hasher, "embed-mtime-unavailable", name),
                        }
                    }
                    Err(e) => hash_field(&mut hasher, "embed-stat-error", &format!("{name}: {e}")),
                }
            }
        }
        Err(e) => hash_field(&mut hasher, "embed-resolve-error", &e.to_string()),
    }
    hex::encode(hasher.finalize())
}

fn default_output_path(args: &CompileArgs) -> PathBuf {
    if let Some(output) = &args.output {
        return output.clone();
    }
    let raw_stem = args
        .input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    let stem = crate::commands::sanitize::sanitize_for_linker_argv(raw_stem);
    // Same helper the compile pipeline links with (#5740) — the cache
    // fingerprints the output file, so a divergence here means it stats a path
    // the build never wrote (Android used to land on the bare stem `app` while
    // the link produced `libapp.so`).
    super::output_path::default_output_path(
        args.output_type == "dylib",
        args.output_type == "staticlib",
        args.target.as_deref(),
        &stem,
    )
}

fn current_env() -> Vec<EnvFingerprint> {
    BUILD_CACHE_ENV_VARS
        .iter()
        .map(|name| EnvFingerprint {
            name: (*name).to_string(),
            value: std::env::var_os(name).map(|v| v.to_string_lossy().into_owned()),
        })
        .collect()
}

fn current_perry_fingerprint() -> std::io::Result<FileFingerprint> {
    let exe = std::env::current_exe()?;
    file_fingerprint(&exe)
}

fn config_inputs_for(
    sources: &[FileFingerprint],
    project_root: &Path,
    cache_root: &Path,
) -> BTreeSet<PathBuf> {
    let mut out = BTreeSet::new();
    for name in ["package.json", "perry.toml", "tsconfig.json", "perry.lock"] {
        let path = project_root.join(name);
        if path.exists() {
            out.insert(path);
        }
        let path = cache_root.join(name);
        if path.exists() {
            out.insert(path);
        }
    }
    for source in sources {
        let mut dir = PathBuf::from(&source.path);
        dir.pop();
        loop {
            for name in ["package.json", "perry.toml"] {
                let candidate = dir.join(name);
                if candidate.exists() {
                    out.insert(candidate);
                }
            }
            if dir == project_root || !dir.pop() {
                break;
            }
        }
    }
    out
}

fn verify_files(files: &[FileFingerprint]) -> Result<(), ()> {
    for expected in files {
        if file_fingerprint_from_str(&expected.path).map_err(|_| ())? != *expected {
            return Err(());
        }
    }
    Ok(())
}

fn file_fingerprint_from_str(path: &str) -> std::io::Result<FileFingerprint> {
    file_fingerprint(Path::new(path))
}

fn file_fingerprint(path: &Path) -> std::io::Result<FileFingerprint> {
    let path_identity = absolute_identity(path);
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut size = 0_u64;
    let mut buf = [0_u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        size += n as u64;
        hasher.update(&buf[..n]);
    }
    Ok(FileFingerprint {
        path: path_identity,
        size,
        sha256: hex::encode(hasher.finalize()),
    })
}

fn absolute_identity(path: &Path) -> String {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    if let Ok(canonical) = absolute.canonicalize() {
        return canonical.to_string_lossy().into_owned();
    }

    absolute
        .parent()
        .and_then(|parent| {
            let file_name = absolute.file_name()?;
            Some(
                parent
                    .canonicalize()
                    .unwrap_or_else(|_| parent.to_path_buf())
                    .join(file_name),
            )
        })
        .unwrap_or(absolute)
        .to_string_lossy()
        .into_owned()
}

fn hash_field(hasher: &mut Sha256, name: &str, value: &str) {
    hasher.update(name.as_bytes());
    hasher.update([0]);
    hasher.update(value.as_bytes());
    hasher.update([0xff]);
}

fn short_hash(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let hex = hex::encode(hasher.finalize());
    hex[..16].to_string()
}
