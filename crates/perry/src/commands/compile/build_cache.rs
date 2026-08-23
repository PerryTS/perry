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
    // exp/llvm-inprocess: selects the in-process LLVM backend for .ll -> .o;
    // a different backend means different object bytes.
    "PERRY_LLVM_INPROCESS",
    "PERRY_WRITE_BARRIERS",
    "PERRY_SHADOW_STACK",
    "PERRY_RS4GC",
    // `-Os` vs `-O3` for every native module.
    "PERRY_LL_SIZE_OPT",
    // The post-RS4GC per-function instruction budget (#8583): a unit that one
    // setting refuses must not be served from a build another accepted.
    "PERRY_LL_RS4GC_MAX_INSTRS",
    // #8583: the relocation estimate above which a function spills its GC roots
    // to a shadow frame. It changes which functions carry statepoints, so it
    // changes the generated code and must be a cache input.
    "PERRY_ROOT_SPILL_RELOCATIONS",
    "PERRY_GC_SAFEPOINT_ONLY",
    "PERRY_INLINE_SHADOW_SLOT",
    "PERRY_DISABLE_BUFFER_FAST_PATH",
    "PERRY_VERIFY_NATIVE_REGIONS",
    // #6125: the resolved CPU baseline (promoted from --march / perry.toml
    // [build] by promote_cpu_baseline_env before this probe runs). Flipping
    // it must invalidate the build-level no-op check, not just per-object
    // cache entries.
    "PERRY_TARGET_CPU",
    "PERRY_NO_AUTO_OPTIMIZE",
    "PERRY_DISABLE_WELL_KNOWN",
    "PERRY_FORCE_WELL_KNOWN",
    // Both switches change native-vs-JavaScript module routing and therefore
    // the linked artifact, not merely diagnostics.
    "PERRY_ALLOW_PERRY_FEATURES",
    "PERRY_REQUIRE_FAITHFUL_BINDINGS",
    // #7183 — audited, not patched one-by-one. #6394's rule is that the object
    // cache keys EVERY codegen env var; #7161 made
    // `PERRY_GC_MOVING_LOOP_POLLS` a compile-time gate (codegen emits or omits
    // `js_gc_loop_safepoint`) without adding it here, and an audit of
    // `perry-codegen`'s `env::var("PERRY_*")` reads found 35 in the same state.
    // Each of these changes the EMITTED CODE, so a build-level no-op probe that
    // ignores them is one `-o` collision away from handing back a binary built
    // under a different configuration. `moving_loop_polls` is the one that must
    // not go dark: it is the only configuration exercising the evacuating minor
    // end to end.
    //
    // Adding a key can only make the probe MORE conservative, so the bias here
    // is inclusion; the deliberate exclusions are listed in
    // `codegen_env_vars_are_build_cache_inputs` below.
    "PERRY_GC_MOVING_LOOP_POLLS",
    "PERRY_CANONICAL_I32_LOCALS",
    "PERRY_CANONICAL_STR_LOCALS",
    "PERRY_CODEGEN_UNITS",
    "PERRY_CODEGEN_UNIT_BYTES",
    "PERRY_CODEGEN_UNIT_SIZE",
    "PERRY_ENTRY_SYMBOL",
    "PERRY_FULL_OUTLINE_IC",
    "PERRY_FULL_OUTLINE_IC_MIN_FUNCS",
    "PERRY_INLINE_HOT_SMALL",
    "PERRY_INLINE_HOT_SMALL_CAP",
    "PERRY_INLINE_HOT_SMALL_MAX_SITES",
    "PERRY_INLINE_HOT_SMALL_THRESHOLD",
    "PERRY_INLINE_NONBIGINT_BITWISE",
    "PERRY_INT_VALUED_LOCALS",
    "PERRY_JSCVT",
    "PERRY_LD",
    "PERRY_LLVM_LIB",
    "PERRY_LLVM_OPT",
    "PERRY_OUTLINE_METHOD_DISPATCH",
    // Entry-function outlining changes the emitted function graph; the chunk
    // size changes the split points, so both belong in the cache key.
    "PERRY_OUTLINE_ENTRY",
    "PERRY_OUTLINE_ENTRY_CHUNK_STMTS",
    "PERRY_PTR_NUMARRAY_LOCALS",
    "PERRY_PTR_SHAPE_LOCALS",
    "PERRY_PTR_SHAPE_THIS",
    "PERRY_SPECIALIZED_ABI",
    "PERRY_SPECIALIZED_ABI_MAX",
    "PERRY_SPEC_PRESERVE_NONE",
    "PERRY_STATIC_STRING_LOWERING",
    "PERRY_STRING_INIT_CHUNK_SIZE",
    "PERRY_TA_PARAM_F64_READ",
    "PERRY_WATCHOS_ARM64_32",
    // #8105 — number-by-construction locals (see the collector of the same
    // name); `=0` empties the fact and changes every affected function's IR.
    "PERRY_NUMBER_BY_CONSTRUCTION",
];

/// #7183: codegen env vars that deliberately do NOT key the build cache.
///
/// Each either produces a side artifact without changing the emitted object, or
/// varies per-invocation for reasons unrelated to output — keying on those
/// would thrash the cache rather than protect it. Anything not listed here must
/// be a build-cache input; `codegen_env_vars_are_build_cache_inputs` enforces
/// that, so a new compile-time gate cannot repeat #7161's omission silently.
#[cfg(test)]
const BUILD_CACHE_ENV_EXCLUSIONS: &[&str] = &[
    // Diagnostics: emit an extra file / extra stderr, same object bytes.
    "PERRY_SAVE_LL",
    "PERRY_LLVM_DIFF_DIR",
    "PERRY_REPSEL_DEBUG",
    "PERRY_STATEPOINT_REPORT",
    // `opt_report`'s own module doc states the contract this exclusion rests
    // on: "Observational only. Nothing in this module is read by codegen …
    // the returned fact sets are bit-identical with the report on and off,
    // which the CLI's byte-identical-object test asserts." Found by the test
    // below rather than by the audit that preceded it, which is the point of
    // having the test.
    "PERRY_OPT_REPORT",
    // Parallelism only — partitioning is keyed by PERRY_CODEGEN_UNIT_SIZE /
    // PERRY_CODEGEN_UNIT_BYTES / PERRY_CODEGEN_UNITS, which ARE inputs; the
    // job count just decides how many threads chew through the same units.
    "PERRY_CODEGEN_UNIT_JOBS",
    // Human-facing telemetry only; never changes IR or object bytes.
    "PERRY_CODEGEN_PROGRESS",
    "PERRY_CODEGEN_UNIT_TIMINGS",
    // Entry outlining report output is observational only.
    "PERRY_OUTLINE_ENTRY_REPORT",
];

#[cfg(test)]
mod tests {
    use super::{
        absolute_identity, current_env, current_perry_fingerprint, file_fingerprint,
        file_fingerprint_from_str, BuildCacheManifest, BuildCacheProbe, BUILD_CACHE_ENV_EXCLUSIONS,
        BUILD_CACHE_ENV_VARS, BUILD_CACHE_MANIFEST_VERSION,
    };

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
            assert!(BUILD_CACHE_ENV_VARS.contains(&name), "missing {name}");
        }
    }

    /// #7183 / #6394: every `PERRY_*` env var codegen reads is either a
    /// build-cache input or an explicit, justified exclusion.
    ///
    /// This exists because the list rotted once already and did so silently:
    /// #7161 turned `PERRY_GC_MOVING_LOOP_POLLS` into a compile-time gate and
    /// nothing noticed it was missing here. A list maintained by hand against a
    /// growing set of gates will rot again; scanning the source makes the next
    /// omission a red test instead of a stale comment.
    #[test]
    fn codegen_env_vars_are_build_cache_inputs() {
        use std::collections::BTreeSet;

        let codegen_src = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("crates/ reachable from CARGO_MANIFEST_DIR")
            .join("perry-codegen")
            .join("src");
        assert!(codegen_src.is_dir(), "{} missing", codegen_src.display());

        let mut found: BTreeSet<String> = BTreeSet::new();
        let mut stack = vec![codegen_src];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).expect("readable dir") {
                let path = entry.expect("dir entry").path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                    continue;
                }
                let text = std::fs::read_to_string(&path).expect("readable source");
                // `env::var("PERRY_…")` in either spelling. Deliberately literal:
                // a computed name would not be auditable from here anyway, and
                // there are none today.
                for (idx, _) in text.match_indices("env::var(\"PERRY_") {
                    let rest = &text[idx + "env::var(\"".len()..];
                    if let Some(end) = rest.find('"') {
                        found.insert(rest[..end].to_string());
                    }
                }
            }
        }
        assert!(
            found.len() > 20,
            "scan found only {} vars — the matcher is probably broken, which \
             would make this test vacuous",
            found.len()
        );

        let missing: Vec<&String> = found
            .iter()
            .filter(|name| {
                !BUILD_CACHE_ENV_VARS.contains(&name.as_str())
                    && !BUILD_CACHE_ENV_EXCLUSIONS.contains(&name.as_str())
            })
            .collect();
        assert!(
            missing.is_empty(),
            "these codegen env vars key neither the build cache nor an \
             exclusion (#6394's rule): {missing:?}. Add each to \
             BUILD_CACHE_ENV_VARS, or to BUILD_CACHE_ENV_EXCLUSIONS with a \
             reason it cannot change emitted code."
        );

        // A stale exclusion is also a defect: it claims a var exists and is
        // deliberately unkeyed, when codegen may no longer read it at all.
        let stale: Vec<&&str> = BUILD_CACHE_ENV_EXCLUSIONS
            .iter()
            .filter(|name| !found.contains(&(**name).to_string()))
            .collect();
        assert!(
            stale.is_empty(),
            "these exclusions name vars codegen no longer reads: {stale:?}"
        );
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
    // Asset modules are generated from directory contents before collection.
    // Keep the build-level cache conservative until its manifest fingerprints
    // those directories; object/link caches still apply within the build.
    if !args.asset_module.is_empty() {
        return Err("asset-module".to_string());
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
    // #6952: a cached build reuses the finished binary and never runs codegen,
    // so the report would be empty. Same reasoning as explain-lowering above.
    if args.opt_report.is_some() || std::env::var("PERRY_OPT_REPORT").is_ok() {
        return Err("opt-report".to_string());
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
