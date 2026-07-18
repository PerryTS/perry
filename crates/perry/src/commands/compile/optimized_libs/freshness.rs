use super::*;

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use crate::commands::stdlib_features::{compute_required_features, features_to_cargo_arg};
use crate::OutputFormat;

use super::super::library_search::{find_harmonyos_sdk, harmonyos_cross_env};
use super::super::{find_perry_workspace_root, rust_target_triple, CompilationContext};

pub(crate) fn auto_optimized_archives_are_fresh(
    workspace_root: &Path,
    runtime_path: &Path,
    stdlib_path: &Path,
    tokio_using_bindings: &[(String, String, Option<String>)],
    build_stamp_path: &Path,
    expected_build_stamp: &str,
) -> bool {
    match fs::read_to_string(build_stamp_path) {
        Ok(stamp) if stamp == expected_build_stamp => {}
        _ => return false,
    }

    let Ok(runtime_mtime) = file_modified(runtime_path) else {
        return false;
    };
    let Ok(stdlib_mtime) = file_modified(stdlib_path) else {
        return false;
    };
    let archive_mtime = runtime_mtime.min(stdlib_mtime);

    let mut inputs = vec![
        workspace_root.join("Cargo.toml"),
        workspace_root.join("Cargo.lock"),
        workspace_root.join("crates/perry-runtime"),
        workspace_root.join("crates/perry-stdlib"),
    ];
    for (krate, _lib, _tracking) in tokio_using_bindings {
        inputs.push(workspace_root.join("crates").join(krate));
    }

    for input in inputs {
        if input_newer_than(&input, archive_mtime).unwrap_or(true) {
            return false;
        }
    }
    true
}

/// Cache key for the auto-optimize target dir + build stamp. Hashed into the
/// `target/perry-auto-<hash>` dir name so each (features, panic-mode, target,
/// runtime-gate, version) combination gets its own incremental cache. Kept in
/// one place so `build_optimized_libs` and its freshness tests can never drift.
pub(crate) fn auto_optimized_cache_key(
    feature_arg: &str,
    panic_abort_safe: bool,
    target: Option<&str>,
    ctx: &CompilationContext,
) -> String {
    let target_str = target.unwrap_or("host");
    format!(
        "{}|{}|{}|wasm={}|regex={}|temporal={}|ee={}|url={}|norm={}|seg={}|loc={}|diag={}|dgram={}|dyneval={}|v={}",
        feature_arg,
        panic_abort_safe,
        target_str,
        ctx.needs_wasm_runtime,
        ctx.uses_regex,
        ctx.uses_temporal,
        ctx.uses_event_emitter,
        ctx.uses_url,
        ctx.uses_string_normalize,
        ctx.uses_intl_segmenter,
        ctx.uses_intl_locale,
        ctx.uses_diagnostics,
        ctx.uses_dgram,
        // #6559: dyn-eval presence changes the built archive, so it must
        // key the freshness stamp like every other runtime feature toggle.
        perry_hir::has_deferred_dynamic_code_sites(),
        env!("CARGO_PKG_VERSION"),
    )
}

pub(crate) fn auto_optimized_cross_features(
    ctx: &CompilationContext,
    features: &BTreeSet<&'static str>,
    cli_features: &[String],
) -> Vec<String> {
    let mut cross_features: Vec<String> = vec![
        // perry-runtime's "full" feature gates plugin + os.hostname/homedir.
        // Auto-mode keeps it on so existing behavior is preserved; the
        // panic mode is what shrinks the binary.
        "perry-runtime/full".to_string(),
    ];
    for f in features {
        cross_features.push(format!("perry-stdlib/{}", f));
    }
    // CLI `--features` values that target the runtime (game-loop entry-point
    // shims gated behind `ios-game-loop` / `watchos-game-loop` in
    // `perry-runtime/Cargo.toml`) need `perry-runtime/<f>` passed through, not
    // `perry-stdlib/<f>` — they gate a Rust module, not an npm dep surface.
    for f in cli_features {
        if f == "ios-game-loop" || f == "watchos-game-loop" || f == "ohos-napi" {
            cross_features.push(format!("perry-runtime/{}", f));
        }
    }
    // Issue #76 — enable perry-runtime's `wasm-host` feature when the
    // program references `WebAssembly.*`. Without this the shim TU stays
    // out of libperry_runtime.a, so unrelated programs don't drag in
    // unresolved `perry_wasm_host_*` references at link time.
    if ctx.needs_wasm_runtime {
        cross_features.push("perry-runtime/wasm-host".to_string());
    }
    // Binary-size feature gating (kept in sync with the inline list on `main`):
    // each engine/table is linked only when the program actually uses it.
    if ctx.uses_regex {
        cross_features.push("perry-runtime/regex-engine".to_string());
    }
    if ctx.uses_temporal {
        cross_features.push("perry-runtime/temporal".to_string());
    }
    if ctx.uses_url {
        cross_features.push("perry-runtime/url-engine".to_string());
    }
    if ctx.uses_string_normalize {
        cross_features.push("perry-runtime/string-normalize".to_string());
    }
    if ctx.uses_intl_segmenter {
        cross_features.push("perry-runtime/intl-segmenter".to_string());
    }
    if ctx.uses_intl_locale {
        cross_features.push("perry-runtime/intl-locale".to_string());
    }
    if ctx.uses_intl_datetime {
        cross_features.push("perry-runtime/intl-datetime".to_string());
    }
    // Cold-path diagnostic JSON serializers (~95 KB incl. the `serde_json`
    // pulled only by them) — enabled only when the program uses a heap-snapshot
    // API or `process.report`. The env-driven GC/typed-feedback dev trace JSON
    // ride this feature, so honor `PERRY_GC_TRACE` too; both stay off in
    // size-optimized binaries by default.
    let gc_trace_requested = std::env::var("PERRY_GC_TRACE")
        .ok()
        .is_some_and(|value| value == "1" || value.eq_ignore_ascii_case("true"));
    if ctx.uses_diagnostics || gc_trace_requested {
        cross_features.push("perry-runtime/diagnostics".to_string());
    }
    if ctx.uses_dgram {
        cross_features.push("perry-runtime/mod-dgram".to_string());
    }
    // #6559: a deferred dynamic-code site (`eval(...)` / `new Function(...)`
    // with a runtime body) means the binary may construct functions from
    // runtime strings — the optimized runtime must carry the dyn-eval
    // interpreter. The generated code of the schema-codegen ecosystem (ajv)
    // also leans on regex literals (`key.replace(/~/g, …)`), so the regex
    // engine rides along even when the program's own source never uses one.
    if perry_hir::has_deferred_dynamic_code_sites() {
        cross_features.push("perry-runtime/dyn-eval".to_string());
        if !ctx.uses_regex {
            cross_features.push("perry-runtime/regex-engine".to_string());
        }
    }
    // Compile OUT perry-runtime's no-op fetch stubs (`js_fetch_with_options` /
    // `js_headers_new` / `js_request_new`, gated `#[cfg(not(feature =
    // "external-fetch-symbols"))]`) whenever the program uses fetch — perry-stdlib's
    // `web-fetch` then supplies the REAL impls. Without this both the stub
    // (perry-runtime) and the real (perry-stdlib) symbols exist; on the fresh build
    // path the stub has won the link and returned garbage the caller derefs ->
    // SIGSEGV in `js_object_get_class_id`. Enabling the feature drops the stubs from
    // libperry_runtime.a so only the real symbols remain.
    if ctx.uses_fetch {
        cross_features.push("perry-runtime/external-fetch-symbols".to_string());
    }
    cross_features
}

/// Content fingerprint of every workspace source tree that lands in the
/// auto-optimized archives: the crates this build compiles (the runtime/stdlib
/// static wrappers and the tokio-using ext crates) plus their transitive
/// workspace path-deps, plus the workspace manifests. Embedded in the build
/// stamp so a `target/perry-auto-<hash>` dir whose archives were built from
/// DIFFERENT sources can never pass the freshness gate. The mtime check alone
/// is blind to that: a cache restore can hand back archives "newer" than a
/// fresh checkout's sources, and Cargo.lock carries no checksum for path deps.
/// #5892 layer 2 — CI's rust-cache-restored stale dir kept linking pre-#5911
/// ext archives; same key-blindness as the #5778-era "warm auto-opt cache
/// ignores perry-ext-http edits" trap.
pub(crate) fn auto_optimized_source_fingerprint(
    workspace_root: &Path,
    tokio_using_bindings: &[(String, String, Option<String>)],
) -> String {
    use sha2::{Digest, Sha256};

    // Seed with the crates the auto-optimize cargo invocation builds directly.
    let mut crates: BTreeSet<String> = [
        "perry-runtime",
        "perry-stdlib",
        "perry-runtime-static",
        "perry-stdlib-static",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    for (krate, _lib, _tracking) in tokio_using_bindings {
        crates.insert(krate.clone());
    }

    // Transitive workspace path-dep closure: any manifest token that names an
    // existing `crates/<name>` directory is treated as a workspace crate whose
    // source is compiled into the archives. Over-approximating (e.g. a feature
    // named like a crate) only hashes extra source — never misses an input.
    let mut queue: Vec<String> = crates.iter().cloned().collect();
    while let Some(name) = queue.pop() {
        let manifest = workspace_root.join("crates").join(&name).join("Cargo.toml");
        let Ok(text) = fs::read_to_string(&manifest) else {
            continue;
        };
        for line in text.lines() {
            let line = line.trim();
            // `perry-foo = { path = ... }` / `perry-foo.workspace = true`
            // dep lines, and `[dependencies.perry-foo]` section headers.
            let candidate =
                if let Some(header) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                    header.rsplit('.').next().unwrap_or("")
                } else {
                    let end = line
                        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
                        .unwrap_or(line.len());
                    &line[..end]
                };
            if candidate.is_empty() || crates.contains(candidate) {
                continue;
            }
            if workspace_root
                .join("crates")
                .join(candidate)
                .join("Cargo.toml")
                .is_file()
            {
                crates.insert(candidate.to_string());
                queue.push(candidate.to_string());
            }
        }
    }

    fn hash_tree(hasher: &mut Sha256, label: &str, path: &Path) {
        let Ok(meta) = fs::metadata(path) else {
            hasher.update(label.as_bytes());
            hasher.update(b"\0missing\0");
            return;
        };
        if meta.is_file() {
            hasher.update(label.as_bytes());
            hasher.update(b"\0");
            match fs::read(path) {
                Ok(bytes) => {
                    hasher.update((bytes.len() as u64).to_le_bytes());
                    hasher.update(&bytes);
                }
                Err(_) => hasher.update(b"unreadable\0"),
            }
            return;
        }
        if !meta.is_dir() {
            return;
        }
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        let mut names: Vec<String> = entries
            .flatten()
            .filter_map(|e| e.file_name().to_str().map(str::to_string))
            // Same exclusions as `input_newer_than`.
            .filter(|n| n != "target" && n != ".git")
            .collect();
        names.sort();
        for name in names {
            hash_tree(hasher, &format!("{label}/{name}"), &path.join(&name));
        }
    }

    let mut hasher = Sha256::new();
    hasher.update(b"perry-auto-src-v1\0");
    hash_tree(
        &mut hasher,
        "Cargo.toml",
        &workspace_root.join("Cargo.toml"),
    );
    hash_tree(
        &mut hasher,
        "Cargo.lock",
        &workspace_root.join("Cargo.lock"),
    );
    for name in &crates {
        hash_tree(
            &mut hasher,
            &format!("crates/{name}"),
            &workspace_root.join("crates").join(name),
        );
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(32);
    for b in &digest[..16] {
        hex.push_str(&format!("{b:02x}"));
    }
    hex
}

pub(crate) fn auto_optimized_build_stamp(
    key_input: &str,
    target: Option<&str>,
    cross_features: &[String],
    tokio_using_bindings: &[(String, String, Option<String>)],
    source_fingerprint: &str,
) -> String {
    let mut stamp = String::new();
    stamp.push_str("perry-auto-optimized-v2\n");
    stamp.push_str("key=");
    stamp.push_str(key_input);
    stamp.push('\n');
    stamp.push_str("target=");
    stamp.push_str(target.unwrap_or("host"));
    stamp.push('\n');
    stamp.push_str("triple=");
    stamp.push_str(rust_target_triple(target).unwrap_or("host"));
    stamp.push('\n');
    stamp.push_str("features=");
    stamp.push_str(&cross_features.join(","));
    stamp.push('\n');
    stamp.push_str("tokio=");
    for (index, (krate, lib, tracking)) in tokio_using_bindings.iter().enumerate() {
        if index > 0 {
            stamp.push(',');
        }
        stamp.push_str(krate);
        stamp.push(':');
        stamp.push_str(lib);
        stamp.push(':');
        stamp.push_str(tracking.as_deref().unwrap_or(""));
    }
    stamp.push('\n');
    // Source-content fingerprint (see `auto_optimized_source_fingerprint`):
    // ties the stamp to WHAT the archives were built from, not just how.
    stamp.push_str("srcs=");
    stamp.push_str(source_fingerprint);
    stamp.push('\n');
    stamp
}

fn input_newer_than(path: &Path, archive_mtime: SystemTime) -> std::io::Result<bool> {
    let meta = fs::metadata(path)?;
    if meta.is_file() {
        return Ok(meta.modified()? > archive_mtime);
    }
    if !meta.is_dir() {
        return Ok(false);
    }

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let child = entry.path();
        let Some(name) = child.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if name == "target" || name == ".git" {
            continue;
        }
        if input_newer_than(&child, archive_mtime)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn file_modified(path: &Path) -> std::io::Result<SystemTime> {
    let meta = fs::metadata(path)?;
    if !meta.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "expected archive file",
        ));
    }
    meta.modified()
}

pub(crate) fn resolve_auto_well_known_libs(
    workspace_root: &Path,
    release_dir: &Path,
    tokio_using_bindings: &[(String, String, Option<String>)],
    target: Option<&str>,
    format: OutputFormat,
) -> Vec<PathBuf> {
    let mut well_known_libs = Vec::new();
    for (krate, lib, _tracking) in tokio_using_bindings {
        let lib_filename =
            super::super::well_known::ext_staticlib_filename(lib, rust_target_triple(target));
        let lib_path = release_dir.join(&lib_filename);
        if lib_path.exists() {
            well_known_libs.push(lib_path);
            continue;
        }

        let fallback = if let Some(triple) = rust_target_triple(target) {
            let triple_path = workspace_root
                .join("target")
                .join(triple)
                .join("release")
                .join(&lib_filename);
            if triple_path.exists() {
                triple_path
            } else {
                workspace_root
                    .join("target")
                    .join("release")
                    .join(&lib_filename)
            }
        } else {
            workspace_root
                .join("target")
                .join("release")
                .join(&lib_filename)
        };
        if fallback.exists() {
            if matches!(format, OutputFormat::Text) {
                eprintln!(
                    "  well-known: rebuild produced no `{}` in {} — \
                     using workspace fallback (CONTEXT panic risk on tokio I/O)",
                    lib_filename,
                    release_dir.display()
                );
            }
            well_known_libs.push(fallback);
        } else if matches!(format, OutputFormat::Text) {
            eprintln!(
                "  well-known: rebuild produced no `{}` for `{}`; \
                 skipping — link will likely fail with unresolved js_* symbols.",
                lib_filename, krate
            );
        }
    }
    well_known_libs
}

/// True if this binding's wrapper crate has its own tokio dependency
/// for I/O (TcpStream, hyper, reqwest, mongodb, sqlx, redis,
/// tokio-tungstenite, lettre, …) and must therefore share a single
/// tokio compilation with perry-stdlib's runtime.
///
/// Closes #507 — when these wrappers are built in a different
/// target-dir than perry-stdlib, each gets its own private copy of
/// tokio's `CONTEXT` thread-local. perry-stdlib's runtime sets one;
/// the wrapper's `Handle::current()` reads the other (empty) one
/// and panics with "there is no reactor running".
///
/// Wrappers that only use perry-ffi's `spawn_blocking` shim (bcrypt,
/// argon2, sharp, …) route their async work through perry-stdlib's
/// tokio and don't need this — their own crate has no tokio dep.
pub(crate) fn binding_needs_shared_tokio(module: &str) -> bool {
    matches!(
        module,
        // Raw TCP / TLS sockets
        "net"
        // WebSocket client/server
        | "ws"
        // HTTP / HTTPS via reqwest/hyper
        | "http"
        | "https"
        | "http2"
        // HTTP clients (reqwest, hyper)
        | "axios"
        | "node-fetch"
        | "fetch"
        // HTTP server (hyper)
        | "fastify"
        // Database drivers (mongodb, sqlx, redis)
        | "mongodb"
        | "pg"
        | "mysql2"
        | "mysql2/promise"
        | "ioredis"
        | "redis"
        // Mail (lettre)
        | "nodemailer"
    )
}
