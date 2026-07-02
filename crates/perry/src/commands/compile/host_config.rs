//! Host configuration loading extracted from `run_with_parse_cache`.
//!
//! Reads `package.json` and `perry.toml` from the project root and merges
//! their settings into `CompilationContext`, applies environment-variable
//! overrides, validates `perry.compilePackages` against the host allowlist
//! (#497), and parses the `[i18n]` / `[google_auth]` blocks from
//! `perry.toml`.
//!
//! Returns the raw `perry.toml` table (handed back so the orchestrator can
//! reuse it for app-metadata extraction without re-reading the file), the
//! resolved `toml` root directory (used by the i18n translation loader and
//! by later .lproj bundle emission), the parsed i18n config, and the loaded
//! per-locale translation tables.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;

use anyhow::Result;
use perry_codegen::FpContractMode;

use crate::OutputFormat;

use super::audit_manifest::allowlist_matches;
use super::{CompilationContext, CompileArgs};

pub(super) fn apply_pkg_and_toml_config(
    args: &CompileArgs,
    project_root: &Path,
    ctx: &mut CompilationContext,
    format: OutputFormat,
) -> Result<(
    Option<perry_transform::i18n::I18nConfig>,
    BTreeMap<String, BTreeMap<String, String>>,
)> {
    let mut fp_contract_explicit = false;

    // #2309: tree-shaking opt-in via env var (checked unconditionally, even
    // with no host package.json). `perry.experiments.treeShake: true` in the
    // package.json is OR'd in below. Default off ⇒ byte-identical to pre-#2309.
    if let Ok(v) = std::env::var("PERRY_TREE_SHAKE") {
        let v = v.trim().to_ascii_lowercase();
        if !matches!(v.as_str(), "" | "0" | "off" | "false" | "no") {
            ctx.tree_shake = true;
        }
    }

    // Read perry.packageAliases from the project's package.json (if present)
    // This allows mapping npm package imports to native Perry packages at compile time.
    // Example: { "@parse/node-apn": "perry-push", "@prisma/client": "perry-prisma" }
    // Walk up from project_root (which is the parent of the entry file) to find package.json.
    let pkg_json_path = {
        let mut dir = project_root.to_path_buf();
        let mut found = None;
        loop {
            let candidate = dir.join("package.json");
            if candidate.exists() {
                found = Some(candidate);
                break;
            }
            if !dir.pop() {
                break;
            }
        }
        found
    };
    if let Some(pkg_json_path) = pkg_json_path {
        if let Ok(content) = fs::read_to_string(&pkg_json_path) {
            if let Ok(pkg) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(aliases) = pkg
                    .get("perry")
                    .and_then(|p| p.get("packageAliases"))
                    .and_then(|a| a.as_object())
                {
                    for (from, to) in aliases {
                        if let Some(to_str) = to.as_str() {
                            match format {
                                OutputFormat::Text => {
                                    println!("  Package alias: {} → {}", from, to_str)
                                }
                                OutputFormat::Json => {}
                            }
                            ctx.package_aliases.insert(from.clone(), to_str.to_string());
                        }
                    }
                }
                // #497: host-controlled allowlists for the two
                // attack surfaces Perry itself introduced over Node:
                // `perry.nativeLibrary` (transitive deps that link
                // arbitrary native code) and `perry.compilePackages`
                // (compiling untrusted TS into the binary). Both
                // arrays default empty (= nothing allowed); patterns
                // accept exact names, scope wildcards (`@scope/*`),
                // or the universal `*` escape hatch.
                if let Some(allow) = pkg.get("perry").and_then(|p| p.get("allow")) {
                    if let Some(arr) = allow.get("nativeLibrary").and_then(|v| v.as_array()) {
                        for entry in arr {
                            if let Some(s) = entry.as_str() {
                                ctx.allow_native_library.push(s.to_string());
                            }
                        }
                    }
                    if let Some(arr) = allow.get("compilePackages").and_then(|v| v.as_array()) {
                        for entry in arr {
                            if let Some(s) = entry.as_str() {
                                ctx.allow_compile_packages.push(s.to_string());
                            }
                        }
                    }
                }
                if let Some(compile_pkgs) = pkg
                    .get("perry")
                    .and_then(|p| p.get("compilePackages"))
                    .and_then(|a| a.as_array())
                {
                    // #497: collect compilePackages entries here but
                    // defer the allowlist check until after env-var
                    // overrides apply (otherwise
                    // `PERRY_ALLOW_PERRY_FEATURES=1` couldn't unblock
                    // a build whose host hasn't opted in via
                    // package.json yet).
                    for pkg_name in compile_pkgs {
                        if let Some(name) = pkg_name.as_str() {
                            match format {
                                OutputFormat::Text => println!("  Compile package: {}", name),
                                OutputFormat::Json => {}
                            }
                            ctx.compile_packages.insert(name.to_string());
                        }
                    }
                }
                // #1680 (Phase 2 of #1677): build-time codegen steps. Each
                // entry is a shell command (or `{ command, label }`) run
                // before module collection so codegen libraries with an
                // eval-free build-time output (`ajv/standalone`, `prisma
                // generate`, …) emit native-compilable source. Read only
                // from the host package.json — never a dependency's — so a
                // transitive dep can't smuggle in a build command (same
                // trust boundary as compilePackages). The run cwd is the
                // host package.json's directory so relative script paths
                // resolve correctly.
                if let Some(steps) = pkg
                    .get("perry")
                    .and_then(|p| p.get("codegen"))
                    .and_then(|v| v.as_array())
                {
                    ctx.codegen_dir = pkg_json_path.parent().map(Path::to_path_buf);
                    for entry in steps {
                        let step = if let Some(cmd) = entry.as_str() {
                            Some(super::CodegenStep {
                                label: None,
                                command: cmd.to_string(),
                            })
                        } else if let Some(obj) = entry.as_object() {
                            obj.get("command").and_then(|c| c.as_str()).map(|cmd| {
                                super::CodegenStep {
                                    label: obj
                                        .get("label")
                                        .and_then(|l| l.as_str())
                                        .map(str::to_string),
                                    command: cmd.to_string(),
                                }
                            })
                        } else {
                            None
                        };
                        if let Some(step) = step {
                            ctx.codegen_steps.push(step);
                        }
                    }
                }
                // perry.fastMath: opt in to LLVM `reassoc` per-instruction
                // FMF flags on f64 ops. Off by default — Perry produces
                // bit-exact f64 with Node. See `docs/src/cli/fast-math.md`.
                if let Some(fm) = pkg
                    .get("perry")
                    .and_then(|p| p.get("fastMath"))
                    .and_then(|v| v.as_bool())
                {
                    ctx.fast_math = fm;
                }
                // perry.cacheDir: directory for Perry's on-disk caches.
                // Default `<root>/node_modules/.cache/perry` (the
                // find-cache-dir convention). Precedence: `--cache-dir` →
                // `PERRY_CACHE_DIR` → perry.toml `[perry] cacheDir` →
                // package.json `perry.cacheDir`. `compile.rs` already resolved
                // `ctx.cache_dir` once before the build-cache probe ran;
                // re-resolve here so the canonical config pass owns the read
                // alongside its `perry.*` siblings and every later consumer
                // (audit.json, object/link cache, sandbox profiles) sees the
                // same value. `cache_dir_override` reads the env + perry.toml +
                // package.json layers; `args.cache_dir` (the CLI flag) wins
                // over all three — same merge `compile.rs` performs.
                // See `docs/src/cli/cache-dir.md`.
                let cache_dir_override = args
                    .cache_dir
                    .clone()
                    .or_else(|| super::object_cache::cache_dir_override(&ctx.cache_root));
                ctx.cache_dir = super::object_cache::resolve_cache_dir(
                    &ctx.cache_root,
                    cache_dir_override.as_deref(),
                );
                // #2309: perry.experiments.treeShake — host opt-in to
                // tree-shaking / dead-code elimination (OR'd with the
                // PERRY_TREE_SHAKE env var checked above).
                if let Some(true) = pkg
                    .get("perry")
                    .and_then(|p| p.get("experiments"))
                    .and_then(|e| e.get("treeShake"))
                    .and_then(|v| v.as_bool())
                {
                    ctx.tree_shake = true;
                }
                // #2309 (Stage 2): perry.define — esbuild-style build-time
                // substitutions. Only `process.env.<NAME>` keys are honored;
                // values are JSON literals (string/bool/number/null). Host
                // package.json only (same trust boundary as compilePackages).
                if let Some(defines) = pkg
                    .get("perry")
                    .and_then(|p| p.get("define"))
                    .and_then(|d| d.as_object())
                {
                    for (key, val) in defines {
                        if let Some(dv) = json_to_define_value(val) {
                            ctx.define.insert(key.clone(), dv);
                        }
                    }
                }
                // perry.fpContract: explicit contraction mode, separated
                // from broad fast-math reassociation.
                if let Some(v) = pkg.get("perry").and_then(|p| p.get("fpContract")) {
                    let s = v
                        .as_str()
                        .ok_or_else(|| anyhow::anyhow!("perry.fpContract must be a string"))?;
                    ctx.fp_contract_mode = parse_fp_contract_mode(s, "perry.fpContract")?;
                    fp_contract_explicit = true;
                }
                // #503: perry.allowDynamicStdlibDispatch — either a boolean
                // (`true` disables the refusal globally) or an array of
                // npm package names allowed to use dynamic dispatch on
                // stdlib namespaces. Absent / `false` keeps the default
                // (refusal active for all packages including the host).
                if let Some(v) = pkg
                    .get("perry")
                    .and_then(|p| p.get("allowDynamicStdlibDispatch"))
                {
                    if let Some(b) = v.as_bool() {
                        ctx.refuse_dynamic_stdlib_dispatch = !b;
                        if b {
                            match format {
                                OutputFormat::Text => println!(
                                    "  Dynamic stdlib dispatch: ALLOWED globally (perry.allowDynamicStdlibDispatch: true)"
                                ),
                                OutputFormat::Json => {}
                            }
                        }
                    } else if let Some(arr) = v.as_array() {
                        for entry in arr {
                            if let Some(name) = entry.as_str() {
                                match format {
                                    OutputFormat::Text => println!(
                                        "  Dynamic stdlib dispatch: ALLOWED for `{}`",
                                        name
                                    ),
                                    OutputFormat::Json => {}
                                }
                                ctx.allow_dynamic_stdlib_packages.insert(name.to_string());
                            }
                        }
                    }
                }
                // #501: host's own package name. Used by the capability
                // walker to identify host code (which gets `*`
                // unconditionally). Falls back to None if the
                // package.json has no `name`.
                if let Some(name) = pkg.get("name").and_then(|n| n.as_str()) {
                    ctx.host_package_name = Some(name.to_string());
                }
                // #501: perry.permissions — host-controlled
                // per-package capability policy.
                //
                //   "perry": {
                //     "permissions": {
                //       "lodash": [],
                //       "axios": ["net:fetch"],
                //       "*": ["crypto"]
                //     }
                //   }
                //
                // Absent → pass disabled (existing builds compile
                // unchanged). Present → strict; every dep stdlib call
                // not in its policy (or the `*` default) fails the
                // build at the offending source span.
                if let Some(perms) = pkg
                    .get("perry")
                    .and_then(|p| p.get("permissions"))
                    .and_then(|v| v.as_object())
                {
                    for (pkg_name, tokens) in perms {
                        let token_list: Vec<String> = match tokens.as_array() {
                            Some(arr) => arr
                                .iter()
                                .filter_map(|t| t.as_str().map(|s| s.to_string()))
                                .collect(),
                            None => continue,
                        };
                        ctx.permissions.insert(pkg_name.clone(), token_list);
                    }
                }
                // #505: per-package opt-out from the build.rs sandbox.
                // See docs/src/cli/sandbox-buildrs.md.
                if let Some(arr) = pkg
                    .get("perry")
                    .and_then(|p| p.get("allowUnsandboxedBuild"))
                    .and_then(|v| v.as_array())
                {
                    for entry in arr {
                        if let Some(s) = entry.as_str() {
                            ctx.allow_unsandboxed_build.push(s.to_string());
                        }
                    }
                }
                // #502: perry.allowedHosts — host-controlled
                // egress allowlist. When set, every literal URL/host
                // in `fetch(...)` and `net.connect(...)` call sites
                // must match a pattern here (exact host,
                // `*.subdomain.example.com`, `https://.../path/*`
                // URL prefix, or `*` universal). When unset, the
                // pass is disabled and existing builds compile
                // unchanged.
                if let Some(arr) = pkg
                    .get("perry")
                    .and_then(|p| p.get("allowedHosts"))
                    .and_then(|v| v.as_array())
                {
                    for entry in arr {
                        if let Some(s) = entry.as_str() {
                            ctx.allowed_hosts.push(s.to_string());
                        }
                    }
                }
                // #504: perry.emitAttest — emit binary attestation
                // sidecar at compile time. See docs/src/cli/emit-attest.md.
                if let Some(ea) = pkg
                    .get("perry")
                    .and_then(|p| p.get("emitAttest"))
                    .and_then(|v| v.as_bool())
                {
                    ctx.emit_attest = ea;
                }
                // #506 — emit kernel sandbox profile alongside the
                // binary. See docs/src/cli/emit-sandbox.md.
                if let Some(es) = pkg
                    .get("perry")
                    .and_then(|p| p.get("emitSandbox"))
                    .and_then(|v| v.as_bool())
                {
                    ctx.emit_sandbox = es;
                }
                // #496: perry.lockdown — refuse the standard
                // arbitrary-code-execution surfaces (perry-jsruntime,
                // perry.nativeLibrary archives, child_process.*).
                if let Some(ld) = pkg
                    .get("perry")
                    .and_then(|p| p.get("lockdown"))
                    .and_then(|v| v.as_bool())
                {
                    ctx.lockdown = ld;
                }
                // #5206: `perry.eval = "defer" | "error"` and/or
                // `perry.strict = true` select strict-eval mode. `perry.eval`
                // wins if both are present (most specific). Unknown
                // `perry.eval` strings are ignored (default keeps "defer").
                if let Some(strict) = pkg
                    .get("perry")
                    .and_then(|p| p.get("strict"))
                    .and_then(|v| v.as_bool())
                {
                    ctx.strict_eval = strict;
                    // #5230: the broad `perry.strict` covers dynamic imports too.
                    ctx.strict_dynamic_import = strict;
                    // #5245: …and recognized-but-unimplemented APIs.
                    ctx.strict_unimplemented = strict;
                }
                if let Some(mode) = pkg
                    .get("perry")
                    .and_then(|p| p.get("eval"))
                    .and_then(|v| v.as_str())
                {
                    match mode {
                        "error" | "strict" => ctx.strict_eval = true,
                        "defer" => ctx.strict_eval = false,
                        _ => {}
                    }
                }
                // #5230: dedicated `perry.dynamicImport = "defer" | "error"`
                // overrides the broad `perry.strict` for dynamic-import sites.
                if let Some(mode) = pkg
                    .get("perry")
                    .and_then(|p| p.get("dynamicImport"))
                    .and_then(|v| v.as_str())
                {
                    match mode {
                        "error" | "strict" => ctx.strict_dynamic_import = true,
                        "defer" => ctx.strict_dynamic_import = false,
                        _ => {}
                    }
                }
                // #502: perry.allowedHosts — compile-time URL/host
                // egress allowlist. Patterns: exact host, "*.foo.com"
                // subdomain wildcard, "https://host/prefix*" URL
                // prefix, or "*" universal escape hatch. Empty list
                // disables the pass entirely.
                if let Some(arr) = pkg
                    .get("perry")
                    .and_then(|p| p.get("allowedHosts"))
                    .and_then(|v| v.as_array())
                {
                    for entry in arr {
                        if let Some(s) = entry.as_str() {
                            ctx.allowed_hosts.push(s.to_string());
                        }
                    }
                }
                if let Some(b) = pkg
                    .get("perry")
                    .and_then(|p| p.get("allowDynamicHosts"))
                    .and_then(|v| v.as_bool())
                {
                    ctx.allow_dynamic_hosts = b;
                }
            }
        }
    }

    // Env var overrides package.json (`PERRY_FAST_MATH=1` opts in).
    if std::env::var("PERRY_FAST_MATH")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        ctx.fast_math = true;
    }
    // CLI flag overrides everything (last wins).
    if args.fast_math {
        ctx.fast_math = true;
    }
    match std::env::var("PERRY_FP_CONTRACT") {
        Ok(v) => {
            ctx.fp_contract_mode = parse_fp_contract_mode(&v, "PERRY_FP_CONTRACT")?;
            fp_contract_explicit = true;
        }
        Err(std::env::VarError::NotPresent) => {}
        Err(e) => return Err(e.into()),
    }
    if let Some(mode) = args.fp_contract.as_deref() {
        ctx.fp_contract_mode = parse_fp_contract_mode(mode, "--fp-contract")?;
        fp_contract_explicit = true;
    }
    if !fp_contract_explicit && ctx.fast_math {
        ctx.fp_contract_mode = FpContractMode::Fast;
    }

    // #503: `PERRY_ALLOW_DYNAMIC_STDLIB=1` disables the dynamic-dispatch
    // refusal pass globally (env var beats package.json). `=0` keeps the
    // refusal on even if the host has opted out, so CI can enforce.
    match std::env::var("PERRY_ALLOW_DYNAMIC_STDLIB") {
        Ok(v) if v == "1" || v.eq_ignore_ascii_case("true") => {
            ctx.refuse_dynamic_stdlib_dispatch = false;
        }
        Ok(v) if v == "0" || v.eq_ignore_ascii_case("false") => {
            ctx.refuse_dynamic_stdlib_dispatch = true;
        }
        _ => {}
    }
    // #503/#5263: the resolved configuration is installed into the HIR
    // thread-locals further below — AFTER lockdown is fully resolved (env +
    // CLI), because lockdown re-arms the dynamic-dispatch refusal that #5263
    // turned off by default. See the install just after the lockdown ladder.

    // #497: `PERRY_ALLOW_PERRY_FEATURES=1` opts every name into both
    // host allowlists at once — emergency escape hatch for builds
    // where editing `package.json` isn't an option (one-off CI run,
    // bisect script, etc.). `=0` enforces the refusal even when
    // `package.json` opted in (fail-closed CI gate).
    match std::env::var("PERRY_ALLOW_PERRY_FEATURES") {
        Ok(v) if v == "1" || v.eq_ignore_ascii_case("true") => {
            if !ctx.allow_native_library.iter().any(|s| s == "*") {
                ctx.allow_native_library.push("*".to_string());
            }
            if !ctx.allow_compile_packages.iter().any(|s| s == "*") {
                ctx.allow_compile_packages.push("*".to_string());
            }
        }
        Ok(v) if v == "0" || v.eq_ignore_ascii_case("false") => {
            ctx.allow_native_library.clear();
            ctx.allow_compile_packages.clear();
        }
        _ => {}
    }

    // #504 — precedence ladder (last wins): package.json
    // `perry.emitAttest` → env `PERRY_EMIT_ATTEST=1` → CLI
    // `--emit-attest`.
    match std::env::var("PERRY_EMIT_ATTEST") {
        Ok(v) if v == "1" || v.eq_ignore_ascii_case("true") => ctx.emit_attest = true,
        Ok(v) if v == "0" || v.eq_ignore_ascii_case("false") => ctx.emit_attest = false,
        _ => {}
    }
    if args.emit_attest {
        ctx.emit_attest = true;
    }

    // #506 — precedence: package.json `perry.emitSandbox` → env
    // `PERRY_EMIT_SANDBOX=1` → CLI `--emit-sandbox` (last wins,
    // mirrors the fast-math ladder).
    match std::env::var("PERRY_EMIT_SANDBOX") {
        Ok(v) if v == "1" || v.eq_ignore_ascii_case("true") => ctx.emit_sandbox = true,
        Ok(v) if v == "0" || v.eq_ignore_ascii_case("false") => ctx.emit_sandbox = false,
        _ => {}
    }
    if args.emit_sandbox {
        ctx.emit_sandbox = true;
    }

    // #496: `--lockdown` precedence: package.json `perry.lockdown` →
    // env `PERRY_LOCKDOWN=1` → CLI `--lockdown` (last wins, mirrors
    // the fast-math knob ladder). `=0` explicitly disables.
    match std::env::var("PERRY_LOCKDOWN") {
        Ok(v) if v == "1" || v.eq_ignore_ascii_case("true") => ctx.lockdown = true,
        Ok(v) if v == "0" || v.eq_ignore_ascii_case("false") => ctx.lockdown = false,
        _ => {}
    }
    if args.lockdown {
        ctx.lockdown = true;
    }

    // #5263: lockdown is the supply-chain gate that re-arms the
    // dynamic-stdlib-dispatch refusal (#503), which is allow-by-default since
    // #5263. An explicit `perry.allowDynamicStdlibDispatch: false` /
    // `PERRY_ALLOW_DYNAMIC_STDLIB=0` already set `refuse_dynamic_stdlib_dispatch`
    // true above; lockdown forces it on regardless. Computed here, after the
    // lockdown ladder is fully resolved (package.json → env → CLI).
    if ctx.lockdown {
        ctx.refuse_dynamic_stdlib_dispatch = true;
    }

    // #503/#5263: install the resolved configuration into the HIR
    // thread-locals before any module lowering begins. Re-installed
    // per-thread by `collect_modules.rs` (rayon workers don't inherit
    // thread-locals), but this set covers the driver thread's own lowering
    // work and serves as documentation of the source of truth.
    perry_hir::set_refuse_dynamic_stdlib_dispatch(ctx.refuse_dynamic_stdlib_dispatch);
    perry_hir::set_allow_dynamic_stdlib_packages(ctx.allow_dynamic_stdlib_packages.clone());

    // #5206 — strict-eval precedence (last wins): package.json
    // `perry.eval`/`perry.strict` (read above) → perry.toml `[perry] eval` /
    // `[perry] strict` → env `PERRY_ALLOW_EVAL=1` (forces OFF, back-compat) →
    // CLI `--strict-eval`. The toml read is folded into the i18n/app-metadata
    // perry.toml parse below; here we apply the env + CLI layers.
    if let Some(dir) = {
        let mut d = Some(project_root.to_path_buf());
        let mut found = None;
        while let Some(dir) = d {
            if dir.join("perry.toml").exists() {
                found = Some(dir);
                break;
            }
            d = dir.parent().map(Path::to_path_buf);
        }
        found
    } {
        if let Some(table) = fs::read_to_string(dir.join("perry.toml"))
            .ok()
            .and_then(|s| s.parse::<toml::Table>().ok())
        {
            if let Some(perry_tbl) = table.get("perry").and_then(|v| v.as_table()) {
                if let Some(strict) = perry_tbl.get("strict").and_then(|v| v.as_bool()) {
                    ctx.strict_eval = strict;
                    // #5230: broad `perry.strict` covers dynamic imports too.
                    ctx.strict_dynamic_import = strict;
                    // #5245: …and recognized-but-unimplemented APIs.
                    ctx.strict_unimplemented = strict;
                }
                if let Some(mode) = perry_tbl.get("eval").and_then(|v| v.as_str()) {
                    match mode {
                        "error" | "strict" => ctx.strict_eval = true,
                        "defer" => ctx.strict_eval = false,
                        _ => {}
                    }
                }
                // #5230: dedicated `perry.dynamicImport` overrides for imports.
                if let Some(mode) = perry_tbl.get("dynamicImport").and_then(|v| v.as_str()) {
                    match mode {
                        "error" | "strict" => ctx.strict_dynamic_import = true,
                        "defer" => ctx.strict_dynamic_import = false,
                        _ => {}
                    }
                }
            }
        }
    }
    // CLI flags opt in.
    if args.strict_eval {
        ctx.strict_eval = true;
    }
    if args.strict_dynamic_import {
        ctx.strict_dynamic_import = true;
    }
    // #5245: `--strict-unimplemented` opts the recognized-but-unimplemented-API
    // gate back into the historical hard `#463` refusal.
    if args.strict_unimplemented {
        ctx.strict_unimplemented = true;
    }
    // Back-compat: `PERRY_ALLOW_EVAL` forces non-strict for a one-off build,
    // overriding any strict flag/config — for both eval and dynamic import
    // (shared AOT escape hatch, #5230).
    if perry_hir::eval_classifier::eval_override_enabled() {
        ctx.strict_eval = false;
        ctx.strict_dynamic_import = false;
    }
    // #5245: `PERRY_ALLOW_UNIMPLEMENTED` is the back-compat escape hatch for the
    // unimplemented-API gate — forces non-strict (defer) for a one-off build.
    if perry_hir::eval_classifier::unimplemented_override_enabled() {
        ctx.strict_unimplemented = false;
    }
    // Install into the HIR thread-local before any lowering begins (re-applied
    // per rayon worker in collect_modules.rs, mirroring the dynamic-stdlib
    // dispatch knob above).
    perry_hir::set_eval_strict_mode(ctx.strict_eval);
    perry_hir::set_unimplemented_strict_mode(ctx.strict_unimplemented);

    // #3527 (blocker #4): materialize `"*"` / `"@scope/*"` wildcard entries
    // in `perry.compilePackages` into concrete installed package names.
    //
    // The *trust* gate (`allowlist_matches`, used for the #497 check below and
    // for `perry.allow.compilePackages`) honors `"*"`, but the *routing*
    // predicates — `is_in_compile_package`, the HIR `COMPILE_PACKAGES_OVERRIDE`
    // lookup, and the many `ctx.compile_packages.contains(name)` sites — all do
    // exact package-name matching. So a literal `"*"` passed the trust gate yet
    // was a silent no-op for compile routing: every dependency still routed to
    // the (removed) JS runtime. Expanding it here makes "compile everything"
    // actually compile everything, and removes the need to hand-enumerate every
    // transitive dependency (the Express tree is 65 packages). This runs after
    // the env-var overrides so `PERRY_ALLOW_PERRY_FEATURES=1` builds expand too.
    let has_universal = ctx.compile_packages.iter().any(|p| p == "*");
    let scope_wildcards: Vec<String> = ctx
        .compile_packages
        .iter()
        .filter_map(|p| p.strip_suffix("/*").map(|s| s.to_string()))
        .collect();
    if has_universal || !scope_wildcards.is_empty() {
        let installed = super::resolve::enumerate_installed_packages(project_root);
        let mut added = 0usize;
        let mut skipped_native = 0usize;
        for name in installed {
            let matches = has_universal
                || scope_wildcards.iter().any(|scope| {
                    name.strip_prefix(scope.as_str())
                        .map(|rest| rest.starts_with('/'))
                        .unwrap_or(false)
                });
            if !matches {
                continue;
            }
            // #5137: don't let the `"*"` / `@scope/*` wildcard sweep in packages
            // Perry ships a native stdlib shim for (commander, dayjs, lru-cache,
            // …). The shim is the supported, optimized path; compiling the real
            // npm source instead routes the import away from the native module
            // table, so hardcoded lowerings like `new Command()` →
            // `js_commander_new` no longer fire and the binding resolves to
            // `undefined` ("TypeError: undefined is not a constructor"). A user
            // who genuinely wants the real source still lists that package
            // explicitly in `perry.compilePackages` — that exact entry is added
            // during package.json parsing and is honored via
            // COMPILE_PACKAGES_OVERRIDE. The wildcard only declines to add it.
            // (`is_native_module` consults COMPILE_PACKAGES_OVERRIDE, but that
            // thread-local is installed later, in collect_modules, so it is empty
            // here and the check reflects the raw NATIVE_MODULES manifest.)
            if perry_hir::is_native_module(&name) {
                skipped_native += 1;
                continue;
            }
            if ctx.compile_packages.insert(name) {
                added += 1;
            }
        }
        // Drop the literal wildcard tokens: they never match a real
        // node_modules path in the routing predicates, so leaving them in would
        // just be dead entries (and `is_in_compile_package` would search for a
        // nonsensical `node_modules/*/` substring).
        ctx.compile_packages
            .retain(|p| p != "*" && !p.ends_with("/*"));
        if let OutputFormat::Text = format {
            println!(
                "  Compile package wildcard: expanded to {} installed package(s)",
                added
            );
            if skipped_native > 0 {
                println!(
                    "  Compile package wildcard: kept {} package(s) on Perry's \
                     native shim (list explicitly to compile the real source)",
                    skipped_native
                );
            }
        }
    }

    // #497: deferred allowlist check for `perry.compilePackages` (the
    // parse loop populated `ctx.compile_packages` above; we validate
    // after env-var overrides). Every entry that flowed in needs a
    // match in `ctx.allow_compile_packages` — the two-key opt-in.
    // Default-empty allowlist = nothing allowed = matches the
    // "greenfield projects: nothing allowed" acceptance bullet.
    for name in ctx.compile_packages.iter() {
        if !allowlist_matches(name, &ctx.allow_compile_packages) {
            anyhow::bail!(
                "package `{name}` is in `perry.compilePackages` but not in \
                 `perry.allow.compilePackages` — compiling untrusted TS into the \
                 binary is a privileged operation and requires explicit host \
                 opt-in. (#497)\n\
                 \n\
                 Review the package, then add it to your host `package.json`:\n\
                 \n\
                   {{\n\
                     \"perry\": {{\n\
                       \"allow\": {{ \"compilePackages\": [\"{name}\"] }}\n\
                     }}\n\
                   }}\n\
                 \n\
                 Scope wildcard (`\"@scope/*\"`) and the universal `\"*\"` escape \
                 hatch are both supported.\n\
                 \n\
                 For a one-off build, set `PERRY_ALLOW_PERRY_FEATURES=1` in the \
                 environment."
            );
        }
    }

    // --- i18n: parse [i18n] config from perry.toml and load locale files ---
    let mut i18n_config: Option<perry_transform::i18n::I18nConfig> = None;
    let mut i18n_translations: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();

    // Walk up from project_root to find perry.toml (it may be in parent of src/)
    let toml_root = {
        let mut dir = project_root.to_path_buf();
        loop {
            if dir.join("perry.toml").exists() {
                break Some(dir);
            }
            if !dir.pop() {
                break None;
            }
        }
    };
    // Parse perry.toml once and reuse — app metadata and the i18n block below
    // both consume it, and a single source-of-truth avoids drift between them.
    let perry_toml: Option<toml::Table> = toml_root.as_deref().and_then(|dir| {
        let path = dir.join("perry.toml");
        fs::read_to_string(&path)
            .ok()
            .and_then(|s| s.parse::<toml::Table>().ok())
    });
    let app_metadata = super::app_metadata::read_app_metadata(
        perry_toml.as_ref(),
        &args.input,
        args.target.as_deref(),
        args.app_bundle_id.as_deref(),
    );
    ctx.app_metadata = app_metadata.clone();
    if let Some(ref toml_dir) = toml_root {
        if let Some(ref doc) = perry_toml {
            if let Some(i18n) = doc.get("i18n").and_then(|v| v.as_table()) {
                let locales: Vec<String> = i18n
                    .get("locales")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();
                let default_locale = i18n
                    .get("default_locale")
                    .and_then(|v| v.as_str())
                    .unwrap_or("en")
                    .to_string();
                let dynamic = i18n
                    .get("dynamic")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                // Parse [i18n.currencies] — locale → currency code
                let mut currencies = HashMap::new();
                if let Some(curr_table) = i18n.get("currencies").and_then(|v| v.as_table()) {
                    for (locale, code) in curr_table {
                        if let Some(code_str) = code.as_str() {
                            currencies.insert(locale.clone(), code_str.to_string());
                        }
                    }
                }

                if !locales.is_empty() {
                    match format {
                        OutputFormat::Text => println!(
                            "  i18n: {} locale(s) [{}], default: {}",
                            locales.len(),
                            locales.join(", "),
                            default_locale
                        ),
                        OutputFormat::Json => {}
                    }

                    // Load locale files
                    let locales_dir = toml_dir.join("locales");
                    for locale in &locales {
                        let locale_file = locales_dir.join(format!("{}.json", locale));
                        if locale_file.exists() {
                            if let Ok(json_content) = fs::read_to_string(&locale_file) {
                                match serde_json::from_str::<BTreeMap<String, String>>(
                                    &json_content,
                                ) {
                                    Ok(translations) => {
                                        match format {
                                            OutputFormat::Text => println!(
                                                "    Loaded locales/{}.json ({} keys)",
                                                locale,
                                                translations.len()
                                            ),
                                            OutputFormat::Json => {}
                                        }
                                        i18n_translations.insert(locale.clone(), translations);
                                    }
                                    Err(e) => {
                                        eprintln!(
                                            "  Warning: Failed to parse locales/{}.json: {}",
                                            locale, e
                                        );
                                    }
                                }
                            }
                        } else {
                            eprintln!("  Warning: Locale file locales/{}.json not found", locale);
                        }
                    }

                    i18n_config = Some(perry_transform::i18n::I18nConfig {
                        locales,
                        default_locale,
                        dynamic,
                        currencies,
                    });
                }
            }

            // --- [google_auth] config parsing (issues #674 / #1138) ---
            //
            // Shape:
            //
            //     [google_auth]
            //     ios_client_id     = "..."
            //     android_client_id = "..."
            //     server_client_id  = "..."
            //     default_scopes    = ["openid", "email", "profile"]
            //
            // The actual injection into the host bundle happens later:
            //   - iOS / macOS: `inject_google_auth_info_plist` writes
            //     `GIDClientID` / `GIDServerClientID` / `GIDDefaultScopes`
            //     into the generated Info.plist; the Swift bridge in
            //     `@perryts/google-auth` reads them at runtime via
            //     `Bundle.main.infoDictionary`.
            //   - Android: `inject_google_auth_strings_xml` (in
            //     `build_and_run_android`) writes the server clientID
            //     into `app/src/main/res/values/google_auth.xml`; the
            //     Kotlin bridge reads it via
            //     `R.string.google_auth_server_client_id`.
            // This block validates types up-front so user typos
            // (`ios_client_id = 42` etc.) surface at compile time
            // instead of waiting for the native SDK to reject them
            // at sign-in time.
            if let Some(ga) = doc.get("google_auth").and_then(|v| v.as_table()) {
                fn warn_non_string(key: &str, ga: &toml::Table) {
                    if let Some(v) = ga.get(key) {
                        if v.as_str().is_none() {
                            eprintln!(
                                "  Warning: perry.toml [google_auth].{} must be a string (got {:?}); ignoring.",
                                key,
                                v.type_str()
                            );
                        }
                    }
                }
                warn_non_string("ios_client_id", ga);
                warn_non_string("android_client_id", ga);
                // #1303 — project-relative search dir for the vendored
                // optional framework (GoogleSignIn SDK). Consumed by the link
                // step (`link/mod.rs::resolve_optional_framework_dir`) and the
                // `perry publish` tarball; validated here only for type.
                warn_non_string("framework_dir", ga);
                warn_non_string("server_client_id", ga);

                if let Some(scopes) = ga.get("default_scopes") {
                    match scopes.as_array() {
                        Some(arr) => {
                            for (i, item) in arr.iter().enumerate() {
                                if item.as_str().is_none() {
                                    eprintln!(
                                        "  Warning: perry.toml [google_auth].default_scopes[{}] must be a string (got {:?}); ignoring.",
                                        i,
                                        item.type_str()
                                    );
                                }
                            }
                        }
                        None => {
                            eprintln!(
                                "  Warning: perry.toml [google_auth].default_scopes must be an array of strings (got {:?}); ignoring.",
                                scopes.type_str()
                            );
                        }
                    }
                }

                if let OutputFormat::Text = format {
                    let configured = ["ios_client_id", "android_client_id", "server_client_id"]
                        .iter()
                        .filter(|k| ga.get(**k).and_then(|v| v.as_str()).is_some())
                        .count();
                    println!(
                        "  google_auth: {} client id(s) configured (#1138)",
                        configured
                    );
                }
            }

            // --- [ads] config parsing (issue #867) ---
            //
            // Shape:
            //
            //     [ads]
            //     ios_app_id     = "ca-app-pub-XXXX~YYYY"
            //     android_app_id = "ca-app-pub-XXXX~ZZZZ"
            //     test_device_ids = ["DEVICE_HASH"]
            //     request_non_personalized_ads_only = false
            //     att_usage_description = "We use your data to show relevant ads."
            //
            // The Google Mobile Ads SDK requires the per-platform App ID to be
            // present in the host manifest *before* the app launches:
            //   - iOS / Mac Catalyst: `inject_ads_info_plist` writes
            //     `GADApplicationIdentifier` (and, when `att_usage_description`
            //     is set, `NSUserTrackingUsageDescription` so the ATT prompt
            //     can be shown) into the generated Info.plist.
            //   - Android: `inject_ads_android_manifest` writes a
            //     `<meta-data android:name="com.google.android.gms.ads.APPLICATION_ID">`
            //     element under `<application>` in AndroidManifest.xml.
            // We validate types up front so a typo (`ios_app_id = 42`) fails at
            // compile time rather than crashing the SDK's `start()` at launch.
            if let Some(ads) = doc.get("ads").and_then(|v| v.as_table()) {
                fn warn_non_string(key: &str, ads: &toml::Table) {
                    if let Some(v) = ads.get(key) {
                        if v.as_str().is_none() {
                            eprintln!(
                                "  Warning: perry.toml [ads].{} must be a string (got {:?}); ignoring.",
                                key,
                                v.type_str()
                            );
                        }
                    }
                }
                fn warn_non_bool(key: &str, ads: &toml::Table) {
                    if let Some(v) = ads.get(key) {
                        if v.as_bool().is_none() {
                            eprintln!(
                                "  Warning: perry.toml [ads].{} must be a boolean (got {:?}); ignoring.",
                                key,
                                v.type_str()
                            );
                        }
                    }
                }
                warn_non_string("ios_app_id", ads);
                warn_non_string("android_app_id", ads);
                warn_non_string("att_usage_description", ads);
                warn_non_bool("request_non_personalized_ads_only", ads);

                if let Some(ids) = ads.get("test_device_ids") {
                    match ids.as_array() {
                        Some(arr) => {
                            for (i, item) in arr.iter().enumerate() {
                                if item.as_str().is_none() {
                                    eprintln!(
                                        "  Warning: perry.toml [ads].test_device_ids[{}] must be a string (got {:?}); ignoring.",
                                        i,
                                        item.type_str()
                                    );
                                }
                            }
                        }
                        None => {
                            eprintln!(
                                "  Warning: perry.toml [ads].test_device_ids must be an array of strings (got {:?}); ignoring.",
                                ids.type_str()
                            );
                        }
                    }
                }

                if let OutputFormat::Text = format {
                    let configured = ["ios_app_id", "android_app_id"]
                        .iter()
                        .filter(|k| ads.get(**k).and_then(|v| v.as_str()).is_some())
                        .count();
                    println!("  ads: {} app id(s) configured (#867)", configured);
                }
            }
        }
    }

    let _ = (perry_toml, toml_root);
    Ok((i18n_config, i18n_translations))
}

fn parse_fp_contract_mode(value: &str, source: &str) -> Result<FpContractMode> {
    FpContractMode::from_str(value.trim()).ok_or_else(|| {
        anyhow::anyhow!(
            "{} must be one of `off`, `on`, or `fast` (got `{}`)",
            source,
            value
        )
    })
}

/// #2309 (Stage 2): convert a `perry.define` JSON value into a [`DefineValue`].
/// Strings/bools/numbers/null are honored; arrays/objects are rejected
/// (returns `None` — too complex to inline-fold safely).
fn json_to_define_value(val: &serde_json::Value) -> Option<super::DefineValue> {
    match val {
        serde_json::Value::String(s) => Some(super::DefineValue::Str(s.clone())),
        serde_json::Value::Bool(b) => Some(super::DefineValue::Bool(*b)),
        serde_json::Value::Number(n) => n.as_f64().map(super::DefineValue::Number),
        serde_json::Value::Null => Some(super::DefineValue::Null),
        _ => None,
    }
}
