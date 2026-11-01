use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use super::compile_env::{compile_env_value, COMPILE_ENV_VARS};

use super::{
    optimized_libs::{
        gc_diagnostics_requested, gc_instruments_requested, gc_trace_requested, hot_diag_requested,
    },
    BuildCacheStats, CompilationContext, CompileArgs, CompileResult, LinkCacheStats,
};

const BUILD_CACHE_MANIFEST_VERSION: u32 = 3;

#[cfg(test)]
mod tests {
    use super::{
        absolute_identity, current_env, current_perry_fingerprint, file_fingerprint,
        file_fingerprint_from_str, BuildCacheManifest, BuildCacheProbe,
        BUILD_CACHE_MANIFEST_VERSION, COMPILE_ENV_VARS,
    };

    #[test]
    fn current_env_fingerprints_runtime_features_in_isolated_processes() {
        const CHILD_MARKER: &str = "PERRY_BUILD_CACHE_FEATURE_TEST_CHILD";
        const EXPECTED: &str = "PERRY_BUILD_CACHE_FEATURE_TEST_EXPECTED";

        if std::env::var_os(CHILD_MARKER).is_some() {
            let expected = std::env::var(EXPECTED).expect("child expected-feature marker");
            let actual = current_env()
                .into_iter()
                .filter_map(|entry| {
                    let name = entry.name.strip_prefix("runtime-feature:")?;
                    Some(format!("{name}={}", entry.value?))
                })
                .collect::<Vec<_>>()
                .join(",");
            assert_eq!(actual, expected);
            return;
        }

        // Isolate environment-mutating cases in child test processes so this
        // remains safe alongside the other parallel build-cache tests.
        let cases = [
            (
                &[][..],
                "gc-instruments=false,hot-diag=false,gc-trace=false,gc-diagnostics=false",
            ),
            (
                &[("PERRY_GC_INSTRUMENTS", "1")][..],
                "gc-instruments=true,hot-diag=true,gc-trace=false,gc-diagnostics=false",
            ),
            (
                &[("PERRY_GC_SCHEDULE_SEED", "1")][..],
                "gc-instruments=true,hot-diag=false,gc-trace=false,gc-diagnostics=false",
            ),
            (
                &[("PERRY_GC_SCHEDULE_SEED", "7")][..],
                "gc-instruments=true,hot-diag=false,gc-trace=false,gc-diagnostics=false",
            ),
            (
                &[("PERRY_IC_DIAG", "1")][..],
                "gc-instruments=false,hot-diag=true,gc-trace=false,gc-diagnostics=false",
            ),
            (
                &[("PERRY_GC_TRACE", "1")][..],
                "gc-instruments=false,hot-diag=false,gc-trace=true,gc-diagnostics=true",
            ),
            (
                &[("PERRY_GC_LOG", "1")][..],
                "gc-instruments=false,hot-diag=false,gc-trace=false,gc-diagnostics=true",
            ),
            (
                &[("PERRY_GC_DIAG", "0")][..],
                "gc-instruments=false,hot-diag=false,gc-trace=false,gc-diagnostics=false",
            ),
        ];

        for (env, expected) in cases {
            let mut child =
                std::process::Command::new(std::env::current_exe().expect("test executable path"));
            child
                .arg("current_env_fingerprints_runtime_features_in_isolated_processes")
                .arg("--nocapture")
                .env_clear()
                .env(CHILD_MARKER, "1")
                .env(EXPECTED, expected);
            for (name, value) in env {
                child.env(name, value);
            }
            let output = child.output().expect("run isolated feature test");
            assert!(
                output.status.success()
                    && String::from_utf8_lossy(&output.stdout).contains("1 passed"),
                "isolated feature test did not pass exactly one test. stdout: {}\nstderr: {}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            );
        }
    }

    /// The build cache must compare against the compiler RUNNING NOW, not the
    /// one that wrote the manifest.
    ///
    /// The bug this pins: the check used to re-fingerprint the path recorded in
    /// the manifest and compare it to the recorded value. That asks "is the
    /// binary I recorded still unchanged?", which is trivially true when a
    /// DIFFERENT perry runs the second build — the recorded binary is sitting
    /// right where it was. The cache then reported `manifest-match`, skipped
    /// the build, and handed back the first compiler's executable while
    /// printing nothing and exiting 0.
    ///
    /// `perry_version` does not cover this: during pass development the
    /// version rarely moves between rebuilds, which is why `perry_build_id`
    /// exists at all (#544).
    #[test]
    fn a_manifest_from_another_compiler_does_not_match_this_one() {
        // Stand in for "the compiler that wrote the manifest": any other file
        // that exists and is not this executable. Its own fingerprint is
        // self-consistent, which is exactly what made the old check pass.
        let other = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        let recorded = file_fingerprint(&other).expect("fingerprint the stand-in");
        assert_eq!(
            file_fingerprint_from_str(&recorded.path).ok(),
            Some(recorded.clone()),
            "precondition: the recorded binary is unchanged on disk, so the OLD \
             check would have passed here — without this the test proves nothing"
        );

        let running = current_perry_fingerprint().expect("fingerprint the test binary");
        assert_ne!(
            running, recorded,
            "a manifest written by a different compiler must not match"
        );
    }

    /// The expression test above pins the two comparisons in isolation, but it
    /// never calls `probe()` — reverting the production call site to the buggy
    /// form leaves it green. This one drives the real decision path, so it is
    /// the one that actually guards the fix.
    ///
    /// Verified by sabotage: restoring
    /// `file_fingerprint_from_str(&manifest.perry_build_id.path)` at the call
    /// site turns this red while the expression test stays green.
    #[test]
    fn a_foreign_build_id_misses_at_the_probe() {
        let dir = tempfile::tempdir().expect("tempdir");
        let input = dir.path().join("in.ts");
        let output = dir.path().join("out.bin");
        let manifest_path = dir.path().join("manifest.json");
        std::fs::write(&input, b"export {}\n").expect("write input");
        std::fs::write(&output, b"binary").expect("write output");

        // A build id belonging to some other compiler: any real file that is
        // not this executable. It is unchanged on disk, which is precisely the
        // condition under which the old self-comparison passed.
        let foreign = file_fingerprint(
            &std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"),
        )
        .expect("fingerprint the foreign build id");
        assert_ne!(
            foreign,
            current_perry_fingerprint().expect("fingerprint the running binary"),
            "precondition: the manifest must claim a DIFFERENT compiler"
        );

        let manifest = BuildCacheManifest {
            version: BUILD_CACHE_MANIFEST_VERSION,
            perry_version: env!("CARGO_PKG_VERSION").to_string(),
            perry_build_id: foreign,
            args_key: "args".to_string(),
            env: current_env(),
            input_path: absolute_identity(&input),
            output_path: absolute_identity(&output),
            target: "native".to_string(),
            compiled_features: Vec::new(),
            sources: Vec::new(),
            config_inputs: Vec::new(),
            runtime_inputs: Vec::new(),
            object_fingerprints: Vec::new(),
            native_modules: 0,
            js_modules: 0,
            output: file_fingerprint(&output).expect("fingerprint output"),
            sidecar_outputs: Vec::new(),
        };
        std::fs::write(
            &manifest_path,
            serde_json::to_string(&manifest).expect("serialize manifest"),
        )
        .expect("write manifest");

        let probe = BuildCacheProbe {
            args_key: "args".to_string(),
            manifest_path,
            output_path: output,
            target_name: "native".to_string(),
            input_path: input,
            project_root: dir.path().to_path_buf(),
            cache_root: dir.path().to_path_buf(),
            eligible: Ok(()),
        };

        let stats = probe.probe();
        assert!(!stats.hit, "a manifest from another compiler must not hit");
        assert_eq!(
            stats.reason, "perry-build-id",
            "must miss on the build id specifically, not incidentally on a later check"
        );
    }

    #[test]
    fn binding_policy_switches_are_build_cache_inputs() {
        for name in [
            "PERRY_ALLOW_PERRY_FEATURES",
            "PERRY_REQUIRE_FAITHFUL_BINDINGS",
        ] {
            assert!(COMPILE_ENV_VARS.contains(&name), "missing {name}");
        }
    }
}

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
    sidecar_outputs: Vec<FileFingerprint>,
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
        // Embed patterns, package hooks, and resource directories are rooted at
        // the walked-up config root, not the entry file's parent directory.
        // Keep cache eligibility/keying aligned with run_pipeline's embed
        // resolution so `src/main.ts --embed ./dist/**` cannot reuse a binary
        // keyed against the nonexistent `src/dist`.
        let eligible = eligibility(args, cache_root);
        Self {
            args_key: args_key(args, &output_path, cache_root),
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
        // Compare against the compiler RUNNING NOW, not the one the manifest
        // was written by. Re-fingerprinting `manifest.perry_build_id.path`
        // asks "is the binary I recorded still unchanged?", which is trivially
        // true whenever a DIFFERENT perry does the second build — its path is
        // not the recorded one, so the recorded binary sits there untouched
        // and the check passes. The cache then hands back the first compiler's
        // executable and skips the build entirely, reporting
        // `"hit": true, "reason": "manifest-match"` and printing nothing.
        //
        // That is not hypothetical: it cost a full false-regression hunt. A
        // probe compiled by a pre-fix perry kept its stale output when
        // recompiled by a fixed one, the fix read as not working, and the
        // phantom bisected onto an unrelated commit. `perry_version` above
        // does not cover it either — during pass development the version
        // rarely moves between rebuilds, which is the whole reason
        // `perry_build_id` exists (#544).
        if current_perry_fingerprint().ok() != Some(manifest.perry_build_id.clone()) {
            return miss("perry-build-id");
        }
        if verify_files(&manifest.sources).is_err() {
            return miss("source");
        }
        // A newly added nested package.json can override a pruning contract.
        // Verifying only previously existing files misses that graph change.
        let config_paths =
            config_inputs_for(&manifest.sources, &self.project_root, &self.cache_root)
                .iter()
                .map(|path| absolute_identity(path))
                .collect::<BTreeSet<_>>();
        let recorded_config_paths = manifest
            .config_inputs
            .iter()
            .map(|input| input.path.clone())
            .collect::<BTreeSet<_>>();
        if config_paths != recorded_config_paths {
            return miss("config-paths");
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
        if verify_files(&manifest.sidecar_outputs).is_err() {
            return miss("native-addon-sidecar");
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
        let mut source_paths = ctx.native_modules.keys().cloned().collect::<BTreeSet<_>>();
        source_paths.extend(ctx.resolve_inputs.iter().cloned());
        // Graph-discovered assets (including `/$bunfs/root/...` literals) are
        // not necessarily modules. Fingerprint their source bytes alongside
        // modules so changing an embedded file cannot reuse a stale binary.
        source_paths.extend(ctx.embedded_assets.iter().map(|(_, path)| path.clone()));
        for addon in ctx.native_addons.values() {
            source_paths.extend(
                super::native_addon_sidecar::addon_payload_files(ctx, addon)
                    .map_err(|_| "native-addon-payload".to_string())?,
            );
        }
        let sources = source_paths
            .iter()
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
        let sidecar_outputs = if ctx.native_addons.is_empty() {
            Vec::new()
        } else {
            let root = super::native_addon_sidecar::sidecar_root(output_path)
                .map_err(|_| "sidecar-output".to_string())?;
            walkdir::WalkDir::new(root)
                .into_iter()
                .filter_map(|entry| entry.ok())
                .filter(|entry| entry.path().is_file())
                .map(|entry| file_fingerprint(entry.path()))
                .collect::<std::io::Result<Vec<_>>>()
                .map_err(|_| "sidecar-output".to_string())?
        };
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
            sidecar_outputs,
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
    if perry_codegen::fallback_diag::enabled() {
        return Err("fallback-diagnostics".to_string());
    }
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
    // Asset modules are generated from directory contents before collection.
    // Keep the build-level cache conservative until its manifest fingerprints
    // those directories; object/link caches still apply within the build.
    if !args.asset_module.is_empty() {
        return Err("asset-module".to_string());
    }
    // Aliases change resolution without appearing in any source file.
    if !args.module_alias.is_empty() {
        return Err("module-alias".to_string());
    }
    if args.enable_wasm_runtime {
        return Err("wasm-runtime".to_string());
    }
    if args.type_check {
        return Err("type-check".to_string());
    }
    if args.print_hir
        || args.trace.is_some()
        || args.focus.is_some()
        || std::env::var("PERRY_COLLECT_ONLY").ok().as_deref() == Some("1")
    {
        return Err("diagnostic-mode".to_string());
    }
    if args.typed_feedback_profile.is_some() || args.typed_feedback_sites.is_some() {
        return Err("typed-feedback-profile".to_string());
    }
    if args.explain_lowering {
        return Err("explain-lowering".to_string());
    }
    // A reach census must run even when an identical finished build exists.
    if std::env::var_os("PERRY_REACH_REPORT").is_some() {
        return Err("reach-report".to_string());
    }
    // So must the retained-source census: its ranges come from codegen.
    if std::env::var_os("PERRY_SOURCE_NEED_REPORT").is_some() {
        return Err("source-need-report".to_string());
    }
    // Keeping all retained source changes the binary without changing inputs.
    if std::env::var_os("PERRY_RETAIN_ALL_SOURCE").is_some() {
        return Err("retain-all-source".to_string());
    }
    // #6952: a cached build reuses the finished binary and never runs codegen,
    // so the report would be empty. Same reasoning as explain-lowering above.
    if args.opt_report.is_some() || std::env::var("PERRY_OPT_REPORT").is_ok() {
        return Err("opt-report".to_string());
    }
    if std::env::var("PERRY_OUTLINE_ENTRY_REPORT").is_ok() {
        return Err("outline-entry-report".to_string());
    }
    // #9847: same reasoning as `opt-report` above. A cached build reuses the
    // finished binary and never lowers HIR, so the native-instance report
    // would print nothing — and nothing is indistinguishable from "no tag was
    // ever registered", which is the reading this diagnostic exists to make
    // impossible.
    if std::env::var("PERRY_NATIVEINST_DIAG").is_ok() {
        return Err("nativeinst-diag".to_string());
    }

    // #9843: same reasoning as `opt-report` above, and the reason it is not
    // optional. A cached build reuses the finished binary and never lowers
    // HIR, so the segment-view counter would print nothing — and "nothing"
    // reads exactly like "the tier never fired", which is the phantom-green
    // this campaign keeps hitting. Excluded from the cache so a zero is a
    // measured zero.
    if std::env::var("PERRY_SEGVIEW_DIAG").is_ok() {
        return Err("segview-diag".to_string());
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
    match super::defines::load(project_root, &args.define) {
        Ok(defines) => hash_field(
            &mut hasher,
            "defines",
            &serde_json::to_string(&defines).unwrap(),
        ),
        Err(error) => hash_field(&mut hasher, "defines-error", &error.to_string()),
    }
    hash_field(&mut hasher, "input", &absolute_identity(&args.input));
    hash_field(&mut hasher, "output", &absolute_identity(output_path));
    if let Some(root) = &args.bunfs_root {
        // Adding/removing/changing loader metadata changes registration code,
        // even when every JS module and embedded byte is otherwise unchanged.
        match fs::read(root.join("unbun-manifest.json")) {
            Ok(bytes) => hash_field(
                &mut hasher,
                "bunfs-loader-manifest",
                &perry_hex::encode(Sha256::digest(bytes)),
            ),
            Err(error) => hash_field(
                &mut hasher,
                "bunfs-loader-manifest-absent",
                &format!("{:?}", error.kind()),
            ),
        }
    }
    // `--function-order` names a file that a new recording rewrites in place:
    // the layout follows its content, not its path.
    if let Some(order) = &args.function_order {
        match fs::read(order) {
            Ok(bytes) => hash_field(
                &mut hasher,
                "function-order",
                &perry_hex::encode(Sha256::digest(bytes)),
            ),
            Err(error) => hash_field(
                &mut hasher,
                "function-order-absent",
                &format!("{:?}", error.kind()),
            ),
        }
    }
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
    perry_hex::encode(hasher.finalize())
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
    let mut env: Vec<EnvFingerprint> = COMPILE_ENV_VARS
        .iter()
        .map(|name| EnvFingerprint {
            name: (*name).to_string(),
            value: compile_env_value(name),
        })
        .collect();
    env.extend(runtime_feature_fingerprint(
        gc_instruments_requested(),
        hot_diag_requested(),
        gc_trace_requested(),
        gc_diagnostics_requested(),
    ));
    env
}

fn runtime_feature_fingerprint(
    gc_instruments: bool,
    hot_diag: bool,
    gc_trace: bool,
    gc_diagnostics: bool,
) -> [EnvFingerprint; 4] {
    [
        ("runtime-feature:gc-instruments", gc_instruments),
        ("runtime-feature:hot-diag", hot_diag),
        ("runtime-feature:gc-trace", gc_trace),
        ("runtime-feature:gc-diagnostics", gc_diagnostics),
    ]
    .map(|(name, enabled)| EnvFingerprint {
        name: name.to_string(),
        value: Some(enabled.to_string()),
    })
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
    for name in [
        "package.json",
        "perry.json",
        "perry.toml",
        "tsconfig.json",
        "perry.lock",
    ] {
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
        out.extend(super::resolve::tsconfig_paths::jsx_config(Path::new(&source.path)).1);
        let mut dir = PathBuf::from(&source.path);
        dir.pop();
        loop {
            // Union of both sides: each change here is a cache KEY, so
            // dropping either entry leaves a stale build when that file moves.
            for name in ["package.json", "perry.json", "perry.toml", "tsconfig.json"] {
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
        sha256: perry_hex::encode(hasher.finalize()),
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
    let hex = perry_hex::encode(hasher.finalize());
    hex[..16].to_string()
}
