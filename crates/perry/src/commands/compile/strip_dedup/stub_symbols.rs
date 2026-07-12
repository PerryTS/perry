//! Localizing perry-runtime's `stdlib_stubs` symbols against the real
//! perry-stdlib implementations — extracted from `strip_dedup.rs`, which had
//! crossed the 2000-line size gate.
//!
//! The standalone runtime archive ships no-op stubs (`js_stdlib_init_dispatch`,
//! the fetch/ws pumps, …) so runtime-only builds still link. When perry-stdlib
//! IS on the link line it provides the real bodies, and the stub copies must be
//! localized out of the runtime archive — otherwise the linker can bind a call
//! to the no-op and silently wedge the pump.

use super::*;

/// Symbols defined by perry-runtime's `stdlib_stubs` module (the
/// `#[cfg(not(feature = "stdlib"))]` no-op fallbacks). The standalone
/// `perry_runtime.lib` ships with these so runtime-only Windows builds still
/// link; perry-stdlib provides the real implementations. Keep in sync with
/// `crates/perry-runtime/src/stdlib_stubs.rs`.
const STDLIB_STUB_SYMBOLS: &[&str] = &[
    // stdlib dispatch (wires the fetch/ws main-thread pumps)
    "js_stdlib_init_dispatch",
    "js_stdlib_process_pending",
    // global fetch / Response / Request / Headers / Blob (#5000)
    "js_fetch_with_options",
    "js_blob_new",
    "js_headers_new",
    "js_headers_init_from_value",
    "js_request_new",
    "js_response_new",
    "js_response_static_json",
    "js_response_static_redirect",
    "js_response_static_error",
    // WebSocket
    "js_ws_connect",
    "js_ws_connect_start",
    "js_ws_send",
    "js_ws_close",
    "js_ws_is_open",
    "js_ws_message_count",
    "js_ws_receive",
    "js_ws_wait_for_message",
    "js_ws_on",
    "js_ws_server_new",
    "js_ws_server_close",
    "js_ws_process_pending",
    // readline (#347)
    "js_readline_set_raw_mode",
    "js_readline_stdin_on",
    "js_readline_stdin_remove_listener",
    "js_readline_stdin_pause",
    "js_readline_stdin_resume",
    "js_readline_stdin_unref",
    "js_readline_stdin_ref",
    "js_readline_stdin_destroy",
];

/// Locate an LLVM binutil, falling back to the directory that holds the
/// resolved `lld-link`. `find_llvm_tool` already covers the env-var /
/// rust-sysroot / PATH cases; a prebuilt install (no Rust toolchain) whose
/// `C:\Program Files\LLVM\bin` is on none of those still resolves lld-link via
/// [`find_lld_link`]'s standard-location probe, and llvm-ar / llvm-nm /
/// llvm-objcopy live right beside it.
fn find_llvm_tool_or_beside_lld(tool: &str) -> Option<PathBuf> {
    if let Some(p) = find_llvm_tool(tool).or_else(|| find_path_tool(tool)) {
        return Some(p);
    }
    let lld = find_lld_link()?;
    let dir = lld.parent()?;
    let candidate = dir.join(format!("{tool}{}", std::env::consts::EXE_SUFFIX));
    candidate.is_file().then_some(candidate)
}

/// #5000 — localize perry-runtime's `stdlib_stubs` no-op symbols in the
/// standalone Windows runtime archive so perry-stdlib's real
/// fetch / WebSocket / readline / dispatch implementations win the link.
///
/// On Windows the standalone `perry_runtime.lib` is linked FIRST so its
/// canonical `js_*` beat the possibly-stale perry-runtime copies bundled in
/// `perry_stdlib.lib` / `perry_ui_windows.lib` (see the runtime-first block in
/// `link/mod.rs` and #880). But the standalone runtime is built WITHOUT the
/// `stdlib` Cargo feature, so it ALSO defines the no-op `stdlib_stubs`
/// symbols; linked first they shadow perry-stdlib's real ones and `fetch()`
/// silently no-ops (`response.json()` then fails with "Invalid response
/// handle").
///
/// We rewrite a temp copy of the runtime archive, removing each
/// [`STDLIB_STUB_SYMBOLS`] entry that the runtime defines AND perry-stdlib
/// also provides from its member's symbol table via `llvm-objcopy
/// --strip-symbol` (COFF's `lld-link`/`llvm-objcopy` reject the ELF/Mach-O
/// `--localize-symbol`/`--weaken-symbol`, but `--strip-symbol` is supported).
/// The stub's `.text` stays in the object but no longer claims the symbol, so
/// lld-link resolves those references from `perry_stdlib.lib` instead and
/// `/OPT:REF` drops the now-unreferenced stub body. Every other runtime symbol
/// keeps its first-definition win. The stdlib cross-check guarantees we never
/// strip a symbol that only the runtime provides, so no reference is left
/// unresolved.
///
/// Best-effort: a missing LLVM tool or any failed sub-step returns the
/// original `runtime_lib` path unchanged, preserving the pre-fix behavior
/// rather than failing the build.
pub(crate) fn localize_stdlib_stub_symbols_for_windows(
    runtime_lib: &Path,
    stdlib_lib: &Path,
) -> PathBuf {
    match try_localize_stdlib_stub_symbols(runtime_lib, stdlib_lib) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[strip-dedup] runtime stdlib-stub localize skipped (non-fatal): {e}");
            runtime_lib.to_path_buf()
        }
    }
}

fn try_localize_stdlib_stub_symbols(runtime_lib: &Path, stdlib_lib: &Path) -> Result<PathBuf> {
    let lib_name = runtime_lib
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("perry_runtime.lib");

    let llvm_ar = find_llvm_tool_or_beside_lld("llvm-ar")
        .or_else(|| find_path_tool("ar"))
        .ok_or_else(|| anyhow::anyhow!("llvm-ar not found"))?;
    let objcopy = find_llvm_tool_or_beside_lld("llvm-objcopy")
        .or_else(|| find_path_tool("objcopy"))
        .ok_or_else(|| anyhow::anyhow!("llvm-objcopy not found"))?;
    let nm = find_llvm_tool_or_beside_lld("llvm-nm")
        .or_else(|| find_path_tool("nm"))
        .ok_or_else(|| anyhow::anyhow!("llvm-nm not found"))?;

    let abs_runtime = std::fs::canonicalize(runtime_lib)?;
    let abs_stdlib = std::fs::canonicalize(stdlib_lib)?;

    let stub_set: std::collections::HashSet<&str> = STDLIB_STUB_SYMBOLS.iter().copied().collect();

    // Per-member symbols of the runtime archive → which members define a stub.
    // Scan the runtime FIRST: the auto-optimize rebuild builds it with the
    // `stdlib` feature (cargo feature unification through perry-stdlib), so it
    // defines no stubs and we bail before the more expensive stdlib scan. Only
    // the prebuilt distribution's standalone runtime (default features) carries
    // them, which is the configuration #5000 reports.
    let runtime_member_syms = collect_archive_symbols_by_member(&nm, &abs_runtime)
        .ok_or_else(|| anyhow::anyhow!("failed to inspect {lib_name} symbols"))?;
    let mut candidates: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for (member, syms) in &runtime_member_syms {
        let hits: Vec<String> = syms
            .iter()
            .filter(|s| stub_set.contains(s.as_str()))
            .cloned()
            .collect();
        if !hits.is_empty() {
            candidates.insert(member.clone(), hits);
        }
    }
    if candidates.is_empty() {
        // Runtime already built without the stubs (e.g. `stdlib` feature on).
        return Ok(runtime_lib.to_path_buf());
    }

    // Cross-check against perry-stdlib: only strip a stub the real stdlib also
    // provides, so we never turn a runtime-only symbol into an undefined ref.
    let stdlib_syms = collect_archive_symbols_flat(&nm, &abs_stdlib);
    if stdlib_syms.is_empty() {
        return Err(anyhow::anyhow!(
            "llvm-nm reported no symbols for {}",
            abs_stdlib.display()
        ));
    }
    let mut to_localize: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for (member, hits) in candidates {
        let mut kept: Vec<String> = hits
            .into_iter()
            .filter(|s| stdlib_syms.contains(s))
            .collect();
        if !kept.is_empty() {
            kept.sort();
            kept.dedup();
            to_localize.insert(member, kept);
        }
    }
    if to_localize.is_empty() {
        // Runtime has stubs but stdlib provides none of them — nothing to do.
        return Ok(runtime_lib.to_path_buf());
    }

    let tmp_base = std::env::temp_dir().join(format!("perry_strip_{}", std::process::id()));
    std::fs::create_dir_all(&tmp_base).ok();
    let extract_dir = tmp_base.join(format!("_{lib_name}_stub_strip_extract"));
    let _ = std::fs::remove_dir_all(&extract_dir);
    std::fs::create_dir_all(&extract_dir)?;
    let trimmed_lib = tmp_base.join(format!("_{lib_name}_stub_stripped.lib"));
    let _ = std::fs::remove_file(&trimmed_lib);

    // Work on a copy and REPLACE only the handful of stub members in place.
    // Extracting every member and rebuilding the whole archive overflows the
    // Windows command-line limit — perry_runtime.lib has hundreds of members
    // (os error 206). `llvm-ar r` rewrites just the named member and keeps the
    // rest untouched.
    std::fs::copy(&abs_runtime, &trimmed_lib)?;

    let mut localized = 0usize;
    for (member, symbols) in &to_localize {
        // Extract just this member next to the copy, strip its stub symbols,
        // then splice it back over the original member.
        let extract_out = Command::new(&llvm_ar)
            .arg("x")
            .arg(&abs_runtime)
            .arg(member)
            .current_dir(&extract_dir)
            .output()?;
        if !extract_out.status.success() {
            let stderr = String::from_utf8_lossy(&extract_out.stderr);
            return Err(anyhow::anyhow!("failed to extract {member}: {stderr}"));
        }
        let member_path = extract_dir.join(member);
        if !member_path.exists() {
            // `llvm-ar x` returned success but produced no file (e.g. a member
            // name that doesn't round-trip as a path). Don't silently skip: that
            // would return a "localized" archive with this member's stubs still
            // global. Fail so the caller falls back to the untouched runtime.
            return Err(anyhow::anyhow!(
                "failed to extract {member}: member file was not created"
            ));
        }
        let mut objcopy_cmd = Command::new(&objcopy);
        for symbol in symbols {
            objcopy_cmd.arg("--strip-symbol").arg(symbol);
        }
        let out = objcopy_cmd.arg(&member_path).output()?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(anyhow::anyhow!(
                "failed to strip stub symbols from {member}: {stderr}"
            ));
        }
        let replace_out = Command::new(&llvm_ar)
            .arg("r")
            .arg(&trimmed_lib)
            .arg(&member_path)
            .output()?;
        if !replace_out.status.success() {
            let stderr = String::from_utf8_lossy(&replace_out.stderr);
            return Err(anyhow::anyhow!("failed to splice {member}: {stderr}"));
        }
        localized += symbols.len();
    }

    // Regenerate the archive symbol index so lld-link no longer sees the
    // stripped stub symbols as provided by the rewritten member(s).
    let index_out = Command::new(&llvm_ar).arg("s").arg(&trimmed_lib).output()?;
    if !index_out.status.success() {
        let stderr = String::from_utf8_lossy(&index_out.stderr);
        return Err(anyhow::anyhow!("failed to reindex {lib_name}: {stderr}"));
    }

    eprintln!(
        "[strip-dedup] {lib_name}: stripped {localized} stdlib-stub symbol def(s) \
         so perry-stdlib wins the link (#5000)"
    );
    let _ = std::fs::remove_dir_all(&extract_dir);
    Ok(trimmed_lib)
}

/// macOS/Linux (#5000) equivalent of [`localize_stdlib_stub_symbols_for_windows`].
///
/// The prebuilt standalone `libperry_runtime.a` is built WITHOUT the `stdlib`
/// Cargo feature, so it defines the no-op `stdlib_stubs` symbols. On the
/// macOS/Linux link line it is linked alongside the auto-optimized perry-stdlib
/// (which carries the REAL `js_fetch_with_options` / `js_headers_new` / `js_ws_*`
/// / `js_readline_*`), and with archive first-definition-wins the runtime stub
/// can satisfy the user's fetch reference first — so `fetch()` silently no-ops
/// (`[perry] warning: js_headers_new is a no-op stub`) and a program awaiting the
/// fetch hangs. Unlike COFF, ELF/Mach-O accept `--localize-symbol`, so localize
/// (global→local) exactly those stub symbols the runtime defines AND perry-stdlib
/// also provides; the now-local stub no longer satisfies the external reference,
/// the linker resolves it from perry-stdlib, and `-dead_strip`/`--gc-sections`
/// drops the unreferenced stub body. The stdlib cross-check guarantees a
/// runtime-only symbol is never localized. Best-effort: returns `runtime_lib`
/// unchanged on any failure, preserving pre-fix behavior.
pub(crate) fn localize_stdlib_stub_symbols(runtime_lib: &Path, stdlib_lib: &Path) -> PathBuf {
    match try_localize_stdlib_stub_symbols_unix(runtime_lib, stdlib_lib) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[strip-dedup] runtime stdlib-stub localize skipped (non-fatal): {e}");
            runtime_lib.to_path_buf()
        }
    }
}

fn try_localize_stdlib_stub_symbols_unix(runtime_lib: &Path, stdlib_lib: &Path) -> Result<PathBuf> {
    let lib_name = runtime_lib
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("libperry_runtime.a");

    // Mach-O requires a matched-LLVM `llvm-objcopy` for `--localize-symbol`
    // (a mismatched one rejects it on Mach-O / `llvm-nm` mis-reads nightly
    // bitcode), so prefer the nightly toolchain tool, mirroring
    // `strip_duplicate_objects_from_well_known_lib`.
    let llvm_ar = find_llvm_tool("llvm-ar")
        .or_else(|| find_path_tool("ar"))
        .ok_or_else(|| anyhow::anyhow!("llvm-ar not found"))?;
    let objcopy = find_nightly_llvm_tool("llvm-objcopy")
        .or_else(|| find_llvm_tool("llvm-objcopy"))
        .or_else(|| find_path_tool("objcopy"))
        .ok_or_else(|| anyhow::anyhow!("llvm-objcopy not found"))?;
    let nm = find_nightly_llvm_tool("llvm-nm")
        .or_else(|| find_llvm_tool("llvm-nm"))
        .or_else(|| find_path_tool("nm"))
        .ok_or_else(|| anyhow::anyhow!("llvm-nm not found"))?;

    let abs_runtime = std::fs::canonicalize(runtime_lib)?;
    let abs_stdlib = std::fs::canonicalize(stdlib_lib)?;

    let stub_set: std::collections::HashSet<&str> = STDLIB_STUB_SYMBOLS.iter().copied().collect();

    let runtime_member_syms = collect_archive_symbols_by_member(&nm, &abs_runtime)
        .ok_or_else(|| anyhow::anyhow!("failed to inspect {lib_name} symbols"))?;
    let mut candidates: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for (member, syms) in &runtime_member_syms {
        let hits: Vec<String> = syms
            .iter()
            .filter(|s| stub_set.contains(s.as_str()))
            .cloned()
            .collect();
        if !hits.is_empty() {
            candidates.insert(member.clone(), hits);
        }
    }
    if candidates.is_empty() {
        // Runtime built without the stubs (e.g. `stdlib` feature on).
        return Ok(runtime_lib.to_path_buf());
    }

    // Cross-check against perry-stdlib: only localize a stub the real stdlib also
    // provides, so we never turn a runtime-only symbol into an undefined ref.
    let stdlib_syms = collect_archive_symbols_flat(&nm, &abs_stdlib);
    if stdlib_syms.is_empty() {
        return Err(anyhow::anyhow!(
            "llvm-nm reported no symbols for {}",
            abs_stdlib.display()
        ));
    }
    let mut to_localize: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for (member, hits) in candidates {
        let mut kept: Vec<String> = hits
            .into_iter()
            .filter(|s| stdlib_syms.contains(s))
            .collect();
        if !kept.is_empty() {
            kept.sort();
            kept.dedup();
            to_localize.insert(member, kept);
        }
    }
    if to_localize.is_empty() {
        return Ok(runtime_lib.to_path_buf());
    }

    let tmp_base = std::env::temp_dir().join(format!("perry_strip_{}", std::process::id()));
    std::fs::create_dir_all(&tmp_base).ok();
    let extract_dir = tmp_base.join(format!("_{lib_name}_stub_localize_extract"));
    let _ = std::fs::remove_dir_all(&extract_dir);
    std::fs::create_dir_all(&extract_dir)?;
    let trimmed_lib = tmp_base.join(format!("_{lib_name}_stub_localized.a"));
    let _ = std::fs::remove_file(&trimmed_lib);
    std::fs::copy(&abs_runtime, &trimmed_lib)?;

    let mut localized = 0usize;
    for (member, symbols) in &to_localize {
        let extract_out = Command::new(&llvm_ar)
            .arg("x")
            .arg(&abs_runtime)
            .arg(member)
            .current_dir(&extract_dir)
            .output()?;
        if !extract_out.status.success() {
            let stderr = String::from_utf8_lossy(&extract_out.stderr);
            return Err(anyhow::anyhow!("failed to extract {member}: {stderr}"));
        }
        let member_path = extract_dir.join(member);
        if !member_path.exists() {
            // `llvm-ar x` returned success but produced no file (e.g. a member
            // name that doesn't round-trip as a path). Don't silently skip: that
            // would return a "localized" archive with this member's stubs still
            // global. Fail so the caller falls back to the untouched runtime.
            return Err(anyhow::anyhow!(
                "failed to extract {member}: member file was not created"
            ));
        }
        let mut objcopy_cmd = Command::new(&objcopy);
        for symbol in symbols {
            objcopy_cmd.arg("--localize-symbol").arg(symbol);
        }
        let out = objcopy_cmd.arg(&member_path).output()?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(anyhow::anyhow!(
                "failed to localize stub symbols in {member}: {stderr}"
            ));
        }
        let replace_out = Command::new(&llvm_ar)
            .arg("r")
            .arg(&trimmed_lib)
            .arg(&member_path)
            .output()?;
        if !replace_out.status.success() {
            let stderr = String::from_utf8_lossy(&replace_out.stderr);
            return Err(anyhow::anyhow!("failed to splice {member}: {stderr}"));
        }
        localized += symbols.len();
    }

    let index_out = Command::new(&llvm_ar).arg("s").arg(&trimmed_lib).output()?;
    if !index_out.status.success() {
        let stderr = String::from_utf8_lossy(&index_out.stderr);
        return Err(anyhow::anyhow!("failed to reindex {lib_name}: {stderr}"));
    }

    eprintln!(
        "[strip-dedup] {lib_name}: localized {localized} stdlib-stub symbol(s) \
         so perry-stdlib wins the link (#5000, macOS/Linux)"
    );
    let _ = std::fs::remove_dir_all(&extract_dir);
    Ok(trimmed_lib)
}

/// Remove from `lib_path` every archive member whose name (a) starts with
/// `name_prefix` and (b) also appears in `reference_lib`, returning the path to
/// a rebuilt archive.
///
/// Used on tier-3 (tvOS/watchOS) to drop the perry-runtime object(s) that the
/// auto-optimized perry-stdlib bundles. The auto-optimizer rebuilds perry-stdlib
/// *and* perry-runtime from the same `-Zbuild-std` crate graph, so a
/// perry-runtime codegen unit (e.g.
/// `perry_runtime-<hash>.perry_runtime.<hash>-cgu.0.rcgu.o`) lands in BOTH
/// archives. They are byte-identical (the hashes in the member name encode the
/// content), and perry-runtime is linked separately right after stdlib, so the
/// stdlib copy is pure duplication. ld64 (Mach-O) has no `/FORCE:MULTIPLE`, so an
/// identical object reachable from two archives is a fatal "duplicate symbol".
///
/// The `name_prefix` filter is essential: only the `perry_runtime-*` members are
/// pure duplication. The `std-*` / `alloc-*` / `core-*` members are also shared,
/// but stdlib's copies are *load-bearing* — being earliest on the link line they
/// satisfy std symbols first, which stops ld64 from pulling the std objects out
/// of perry-runtime AND the bundling native lib for the same symbols (those two
/// would then collide on e.g. `__rdl_alloc`). So we keep stdlib's std/alloc/core
/// objects and strip only its redundant perry-runtime objects.
pub(super) fn strip_members_present_in_reference(
    lib_path: &Path,
    reference_lib: &Path,
    name_prefix: &str,
) -> Result<PathBuf> {
    let lib_name = lib_path.file_name().and_then(|f| f.to_str()).unwrap_or("?");
    let llvm_ar = find_llvm_tool("llvm-ar")
        .or_else(|| find_path_tool("ar"))
        .ok_or_else(|| anyhow::anyhow!("ar not found"))?;

    let abs_lib = std::fs::canonicalize(lib_path)?;
    let abs_ref = std::fs::canonicalize(reference_lib)?;

    let list_members = |archive: &Path| -> Result<Vec<String>> {
        let out = Command::new(&llvm_ar).arg("t").arg(archive).output()?;
        if !out.status.success() {
            return Err(anyhow::anyhow!(
                "failed to list members of {}",
                archive.display()
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| l.to_string())
            .collect())
    };

    let ref_members: std::collections::BTreeSet<String> =
        list_members(&abs_ref)?.into_iter().collect();
    let members = list_members(&abs_lib)?;
    let remove_set: std::collections::BTreeSet<&String> = members
        .iter()
        .filter(|m| m.starts_with(name_prefix) && ref_members.contains(*m))
        .collect();
    if remove_set.is_empty() {
        return Ok(lib_path.to_path_buf());
    }

    let tmp_base = std::env::temp_dir().join(format!("perry_strip_{}", std::process::id()));
    std::fs::create_dir_all(&tmp_base).ok();
    let extract_dir = tmp_base.join(format!("_{lib_name}_refdiff_extract"));
    let _ = std::fs::remove_dir_all(&extract_dir);
    std::fs::create_dir_all(&extract_dir)?;
    let trimmed_lib = tmp_base.join(format!("_{lib_name}_refdiff.lib"));
    let _ = std::fs::remove_file(&trimmed_lib);

    let extract_out = Command::new(&llvm_ar)
        .arg("x")
        .arg(&abs_lib)
        .current_dir(&extract_dir)
        .output()?;
    if !extract_out.status.success() {
        let stderr = String::from_utf8_lossy(&extract_out.stderr);
        return Err(anyhow::anyhow!("failed to extract {lib_name}: {stderr}"));
    }

    let mut ar_cmd = Command::new(&llvm_ar);
    ar_cmd.arg("crs").arg(&trimmed_lib);
    let mut kept = 0usize;
    for member in &members {
        if remove_set.contains(member) {
            continue;
        }
        ar_cmd.arg(extract_dir.join(member));
        kept += 1;
    }
    let ar_out = ar_cmd.output()?;
    if !ar_out.status.success() {
        let stderr = String::from_utf8_lossy(&ar_out.stderr);
        return Err(anyhow::anyhow!(
            "failed to create ref-diff archive for {lib_name}: {stderr}"
        ));
    }
    eprintln!(
        "[strip-dedup] {lib_name}: removed {} member(s) also present in {} (kept {kept})",
        remove_set.len(),
        abs_ref.file_name().and_then(|f| f.to_str()).unwrap_or("?")
    );
    let _ = std::fs::remove_dir_all(&extract_dir);
    Ok(trimmed_lib)
}
