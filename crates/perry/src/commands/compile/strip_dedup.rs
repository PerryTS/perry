//! Trim duplicate objects from a bundling staticlib via symbol-set
//! comparison.
//!
//! Extracted from `compile.rs` (Tier 2.1 of the compiler-improvement
//! plan, v0.5.333). The actual dedup logic was rewritten in v0.5.331
//! (Tier 3.1) to use evidence-based symbol-set comparison instead of
//! the v0.5.319/v0.5.320 name-pattern approach. See the
//! `strip_duplicate_objects_from_lib` doc comment for details on the
//! decision algorithm and the v0.5.320 over-prune incident.

use anyhow::Result;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::{find_library, find_lld_link, find_llvm_tool, find_stdlib_library};

const FORCE_EXCLUDE_SYMBOLS: &[&str] = &["js_stdlib_init_dispatch", "js_stdlib_process_pending"];

const RUST_ALLOCATOR_SYMBOL_PARTS: &[&str] = &[
    "__rust_alloc",
    "__rust_dealloc",
    "__rust_realloc",
    "__rust_alloc_zeroed",
    "__rust_alloc_error_handler",
    "__rust_no_alloc_shim_is_unstable",
    "__rdl_alloc",
    "__rdl_dealloc",
    "__rdl_realloc",
    "__rdl_alloc_zeroed",
    "__rdl_alloc_error_handler",
];

// Panic / unwind runtime shims. On tier-3 Mach-O targets (tvOS/watchOS) with no
// prebuilt std, perry-runtime and perry-stdlib are each built with -Zbuild-std,
// so both bundle std's single-definition panic runtime → `ld64.lld: duplicate
// symbol` for these; localizing them so only one staticlib provides them fixes
// that on Mach-O.
//
// They are NOT localized on ELF (see `object_is_elf` guard in the well-known
// localizer): `--localize-symbol rust_eh_personality` also matches the
// compiler-emitted `DW.ref.rust_eh_personality` (substring), and localizing
// that breaks its PC32 relocation → `relocation R_X86_64_PC32 against undefined
// hidden symbol DW.ref.rust_eh_personality can not be used when making a PIE
// object` at link time. ELF tier-1/2 builds take the panic runtime from the
// prebuilt std (single definition), so there is no duplicate to dedup anyway.
const RUST_PANIC_UNWIND_SYMBOL_PARTS: &[&str] = &[
    "__rust_drop_panic",
    "__rust_foreign_exception",
    "rust_begin_unwind",
    "rust_eh_personality",
    "__rust_abort",
    "rust_panic",
];

/// Panic/unwind personality shims (incl. the compiler-emitted
/// `DW.ref.rust_eh_personality`, which substring-matches `rust_eh_personality`).
/// These must not be `--localize-symbol`'d on ELF — it breaks PIE relocations.
fn is_panic_unwind_symbol(symbol: &str) -> bool {
    RUST_PANIC_UNWIND_SYMBOL_PARTS
        .iter()
        .any(|part| symbol.contains(part))
}

fn force_localize_symbol(symbol: &str) -> bool {
    FORCE_EXCLUDE_SYMBOLS.contains(&symbol)
        || RUST_ALLOCATOR_SYMBOL_PARTS
            .iter()
            .any(|part| symbol.contains(part))
        || is_panic_unwind_symbol(symbol)
}

/// True if `path` is an ELF object file (first four bytes `0x7F 'E' 'L' 'F'`).
/// Used to skip panic/unwind-symbol localization on ELF, where localizing
/// `rust_eh_personality` / `DW.ref.rust_eh_personality` breaks PIE relocations
/// (see [`RUST_PANIC_UNWIND_SYMBOL_PARTS`]).
fn object_is_elf(path: &Path) -> bool {
    use std::io::Read;
    let mut magic = [0u8; 4];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut magic))
        .map(|_| magic == [0x7f, b'E', b'L', b'F'])
        .unwrap_or(false)
}

fn find_path_tool(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}

/// Find an LLVM tool shipped with a `nightly` rustup toolchain.
///
/// Tier-3 targets (tvOS/watchOS) build runtime/stdlib with nightly Rust via
/// `-Zbuild-std`, emitting object bitcode from nightly's bundled LLVM (e.g.
/// LLVM 22). A system `llvm-nm` / `llvm-objcopy` from an older LLVM (e.g. 18)
/// fails on that bitcode — `llvm-nm` reports zero symbols ("Unknown attribute
/// kind"), defeating the symbol-set dedup, and `llvm-objcopy` rejects
/// `--localize-symbol` on Mach-O ("option is not supported for MachO"). Prefer
/// nightly's own tool, whose LLVM matches the bytes it produced.
///
/// `$HOME` / `$RUSTUP_HOME` may both be unset (the Linux build worker runs
/// `perry compile` as a systemd subprocess whose environment carries only
/// `PATH`), so the rustup home is also derived from the `rustup`/`cargo` binary
/// on `PATH` and from well-known absolute locations.
fn find_nightly_llvm_tool(tool: &str) -> Option<PathBuf> {
    let exe_suffix = std::env::consts::EXE_SUFFIX;
    let mut rustup_homes: Vec<PathBuf> = Vec::new();
    if let Some(rustup_home) = std::env::var_os("RUSTUP_HOME") {
        rustup_homes.push(PathBuf::from(rustup_home));
    }
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        rustup_homes.push(PathBuf::from(home).join(".rustup"));
    }
    // `<dir>/.cargo/bin/cargo` on PATH implies the rustup home is `<dir>/.rustup`.
    for tool_name in ["rustup", "cargo"] {
        if let Some(bin) = find_path_tool(tool_name) {
            if let Some(cargo_root) = bin
                .parent()
                .and_then(|p| p.parent())
                .and_then(|p| p.parent())
            {
                rustup_homes.push(cargo_root.join(".rustup"));
            }
        }
    }
    for fixed in ["/root/.rustup", "/usr/local/rustup", "/opt/rust/rustup"] {
        rustup_homes.push(PathBuf::from(fixed));
    }

    let mut roots: Vec<PathBuf> = Vec::new();
    for home in rustup_homes {
        let t = home.join("toolchains");
        if !roots.contains(&t) {
            roots.push(t);
        }
    }
    for toolchains in roots {
        let Ok(dir) = std::fs::read_dir(&toolchains) else {
            continue;
        };
        for entry in dir.flatten() {
            if !entry.file_name().to_string_lossy().starts_with("nightly") {
                continue;
            }
            let rustlib = entry.path().join("lib").join("rustlib");
            if let Ok(targets) = std::fs::read_dir(&rustlib) {
                for t in targets.flatten() {
                    let candidate = t.path().join("bin").join(format!("{tool}{exe_suffix}"));
                    if candidate.is_file() {
                        return Some(candidate);
                    }
                }
            }
        }
    }
    None
}

/// Parse `nm --defined-only` archive output into a per-member symbol map.
///
/// LLVM `--format=just-symbols` output shape:
/// ```text
/// member1.o:
/// SYM_A
/// SYM_B
///
/// member2.o:
/// SYM_C
/// ```
/// Lines ending in `:` start a member; subsequent non-empty lines are
/// symbol names. GNU `nm --format=bsd` uses the same member headers but
/// includes address/type fields before each symbol. Some nm versions wrap
/// the header as `archive.a(member.o):` — we strip the parens so the map is
/// keyed off the bare member name, matching `ar t` output.
fn parse_nm_archive_output(
    nm_stdout: &str,
) -> std::collections::HashMap<String, std::collections::HashSet<String>> {
    let mut map: std::collections::HashMap<String, std::collections::HashSet<String>> =
        std::collections::HashMap::new();
    let mut current: Option<String> = None;
    for line in nm_stdout.lines() {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(raw) = trimmed.strip_suffix(':') {
            let member = if let (Some(open), Some(close)) = (raw.rfind('('), raw.rfind(')')) {
                if open < close {
                    raw[open + 1..close].to_string()
                } else {
                    raw.to_string()
                }
            } else {
                raw.to_string()
            };
            current = Some(member);
        } else if let Some(ref m) = current {
            map.entry(m.clone())
                .or_default()
                .insert(parse_nm_symbol_line(trimmed).to_string());
        }
    }
    map
}

fn parse_nm_symbol_line(line: &str) -> &str {
    let mut parts = line.split_whitespace();
    let first = parts.next().unwrap_or(line);
    let second = parts.next();
    if let Some(value) = second {
        if is_nm_symbol_type(value) {
            return parts.next().unwrap_or(line);
        }
    }
    if is_nm_symbol_type(first) {
        return second.unwrap_or(line);
    }
    line
}

fn is_nm_symbol_type(value: &str) -> bool {
    value.len() == 1 && value.as_bytes()[0].is_ascii_alphabetic()
}

/// Run `nm --defined-only` on an archive and parse the output into a
/// per-member symbol map. Returns `None` if the nm invocation fails so callers
/// can fall back to the legacy name-pattern path.
fn collect_archive_symbols_by_member(
    llvm_nm: &Path,
    archive: &Path,
) -> Option<std::collections::HashMap<String, std::collections::HashSet<String>>> {
    let out = Command::new(llvm_nm)
        .arg("--defined-only")
        .arg("--format=bsd")
        .arg(archive)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let parsed = parse_nm_archive_output(&String::from_utf8_lossy(&out.stdout));
    if !parsed.is_empty() {
        return Some(parsed);
    }

    let out = Command::new(llvm_nm)
        .arg("--defined-only")
        .arg("--format=just-symbols")
        .arg(archive)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(parse_nm_archive_output(&String::from_utf8_lossy(
        &out.stdout,
    )))
}

/// Flat union of every symbol defined anywhere in the archive.
fn collect_archive_symbols_flat(
    llvm_nm: &Path,
    archive: &Path,
) -> std::collections::HashSet<String> {
    collect_archive_symbols_by_member(llvm_nm, archive)
        .map(|by_member| by_member.into_values().flatten().collect())
        .unwrap_or_default()
}

/// Flat union of every *external* symbol defined anywhere in the archive.
/// Distinct from [`collect_archive_symbols_flat`], which includes local
/// definitions: a local definition cannot satisfy a cross-object reference,
/// so callers deciding "can this reference resolve from that archive
/// instead?" must use this variant.
fn collect_archive_global_symbols_flat(
    llvm_nm: &Path,
    archive: &Path,
) -> std::collections::HashSet<String> {
    let out = match Command::new(llvm_nm)
        .arg("--defined-only")
        .arg("--extern-only")
        .arg("--format=bsd")
        .arg(archive)
        .output()
    {
        Ok(out) if out.status.success() => out,
        _ => return Default::default(),
    };
    parse_nm_archive_output(&String::from_utf8_lossy(&out.stdout))
        .into_values()
        .flatten()
        .collect()
}

/// Run `nm --undefined-only` on an archive and parse the output into a
/// per-member map of the symbols each member *references* but does not define.
/// Same parse as [`collect_archive_symbols_by_member`]; returns `None` if nm
/// fails so callers can fall back to keeping the archive untouched.
fn collect_archive_undefined_by_member(
    llvm_nm: &Path,
    archive: &Path,
) -> Option<std::collections::HashMap<String, std::collections::HashSet<String>>> {
    let out = Command::new(llvm_nm)
        .arg("--undefined-only")
        .arg("--format=bsd")
        .arg(archive)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(parse_nm_archive_output(&String::from_utf8_lossy(
        &out.stdout,
    )))
}

/// On Windows, build a trimmed UI lib using the rlib (not staticlib).
///
/// perry-ui-windows builds as both rlib and staticlib. The staticlib bundles
/// ALL transitive deps (std, alloc, core, perry-runtime -- 314 objects).
/// perry-stdlib also bundles these. Linking both causes hundreds of duplicate
/// symbols, and /FORCE:MULTIPLE produces corrupt binaries.
///
/// The rlib contains only the UI crate's own code (1 object). We extract it
/// and combine with UI-only deps (windows, serde, regex...) from the staticlib.
/// All shared deps come from perry-stdlib. No /FORCE:MULTIPLE needed.
///
/// **Dedup decision** (Tier 3.1, v0.5.331): when `llvm-nm` is available, drop a
/// staticlib member only if **every** defined symbol it carries is also
/// defined in (a) the rlib (when present) or (b) one of the standalone
/// `libperry_stdlib.a` / `libperry_runtime.a` archives. Members with any
/// unique symbol — typical for crate-specific generic monomorphizations
/// like `hashbrown::raw::RawTable<HashMap<i64, gtk4::Widget>>::reserve_rehash`
/// — are kept. The previous name-pattern approach (e.g. `m.contains(
/// "perry_runtime-")`) was evidence-free and over-pruned on Linux when the
/// bundling staticlib carried unique CGUs (#181 part B). Falls back to the
/// legacy name-pattern when `llvm-nm` isn't installed.
pub(super) fn strip_duplicate_objects_from_lib(lib_path: &PathBuf) -> Result<PathBuf> {
    let lib_name = lib_path.file_name().and_then(|f| f.to_str()).unwrap_or("?");
    eprintln!("[strip-dedup] Processing: {}", lib_path.display());

    let llvm_ar = match find_llvm_tool("llvm-ar").or_else(|| find_path_tool("ar")) {
        Some(ar) => {
            eprintln!("[strip-dedup] ar found: {}", ar.display());
            ar
        }
        None => {
            eprintln!("[strip-dedup] ar not found, skipping dedup for {lib_name}");
            return Err(anyhow::anyhow!("ar not found"));
        }
    };

    // Canonicalize the staticlib path
    let abs_staticlib = std::fs::canonicalize(lib_path)?;

    // List staticlib members
    let staticlib_out = Command::new(&llvm_ar)
        .arg("t")
        .arg(&abs_staticlib)
        .output()?;
    let staticlib_members: Vec<String> = String::from_utf8_lossy(&staticlib_out.stdout)
        .lines()
        .map(|l| l.to_string())
        .collect();
    eprintln!(
        "[strip-dedup] {lib_name}: {} total members",
        staticlib_members.len()
    );

    // Determine library naming convention from the input lib
    let is_win_lib = lib_name.ends_with(".lib");
    let (stdlib_name, runtime_name) = if is_win_lib {
        ("perry_stdlib.lib", "perry_runtime.lib")
    } else {
        ("libperry_stdlib.a", "libperry_runtime.a")
    };
    // Determine target for find_stdlib_library / find_library search
    let search_target: Option<&str> = if is_win_lib {
        Some("windows")
    } else if lib_name.contains("_ios") {
        Some("ios")
    } else if lib_name.contains("_visionos") {
        Some("visionos")
    } else if lib_name.contains("_tvos") {
        Some("tvos")
    } else if lib_name.contains("_watchos") {
        Some("watchos")
    } else {
        None
    };

    // Find perry-stdlib members so we can compute the set difference.
    let stdlib_path = lib_path
        .parent()
        .map(|p| p.join(stdlib_name))
        .filter(|p| p.exists())
        .or_else(|| find_stdlib_library(search_target));

    let mut exclude_members: std::collections::HashSet<String> = std::collections::HashSet::new();

    if let Some(ref sp) = stdlib_path {
        let abs_sp = std::fs::canonicalize(sp).unwrap_or(sp.clone());
        if let Ok(out) = Command::new(&llvm_ar).arg("t").arg(&abs_sp).output() {
            let count_before = exclude_members.len();
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                exclude_members.insert(line.to_string());
            }
            eprintln!(
                "[strip-dedup] {stdlib_name} found: {} — {} members loaded",
                abs_sp.display(),
                exclude_members.len() - count_before
            );
        } else {
            eprintln!(
                "[strip-dedup] WARNING: failed to list {stdlib_name} at {}",
                abs_sp.display()
            );
        }
    } else {
        eprintln!("[strip-dedup] WARNING: {stdlib_name} not found (searched next to lib and via find_stdlib_library)");
    }

    // Also find perry_runtime members
    let runtime_path = lib_path
        .parent()
        .map(|p| p.join(runtime_name))
        .filter(|p| p.exists())
        .or_else(|| find_library(runtime_name, search_target));

    if let Some(ref rp) = runtime_path {
        let abs_rp = std::fs::canonicalize(rp).unwrap_or(rp.clone());
        if let Ok(out) = Command::new(&llvm_ar).arg("t").arg(&abs_rp).output() {
            let count_before = exclude_members.len();
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                exclude_members.insert(line.to_string());
            }
            eprintln!(
                "[strip-dedup] {runtime_name} found: {} — {} members loaded",
                abs_rp.display(),
                exclude_members.len() - count_before
            );
        } else {
            eprintln!(
                "[strip-dedup] WARNING: failed to list {runtime_name} at {}",
                abs_rp.display()
            );
        }
    } else {
        eprintln!("[strip-dedup] WARNING: {runtime_name} not found (searched next to lib and via find_library)");
    }

    eprintln!(
        "[strip-dedup] Total exclude set: {} members from stdlib+runtime .lib files",
        exclude_members.len()
    );

    // Try to find the rlib alongside the staticlib
    // .lib → lib<name>.rlib, .a (already has lib prefix) → lib<name>.rlib
    let rlib_name = lib_path
        .file_name()
        .and_then(|f| f.to_str())
        .map(|f| {
            if f.ends_with(".lib") {
                format!("lib{}", f.replace(".lib", ".rlib"))
            } else {
                // .a files: libfoo.a → libfoo.rlib
                f.replace(".a", ".rlib")
            }
        })
        .unwrap_or_default();
    let rlib_path = lib_path.with_file_name(&rlib_name);
    let has_rlib = rlib_path.exists();
    eprintln!(
        "[strip-dedup] rlib {}: {}",
        if has_rlib { "found" } else { "NOT found" },
        rlib_path.display()
    );

    let rlib_objects: Vec<String> = if has_rlib {
        let abs_rlib = std::fs::canonicalize(&rlib_path)?;
        let rlib_out = Command::new(&llvm_ar).arg("t").arg(&abs_rlib).output()?;
        let objs: Vec<String> = String::from_utf8_lossy(&rlib_out.stdout)
            .lines()
            .filter(|l| l.ends_with(".o"))
            .map(|l| l.to_string())
            .collect();
        eprintln!("[strip-dedup] rlib has {} .o members", objs.len());
        objs
    } else {
        Vec::new()
    };

    // Determine the UI crate name from the staticlib filename
    let _ui_crate_name = lib_path.file_stem().and_then(|f| f.to_str()).unwrap_or("");

    // Filter: keep only objects unique to this lib.
    //
    // **Symbol-set comparison** (Tier 3.1): when `llvm-nm` is available,
    // build the union of symbols provided by (a) the rlib (which we
    // extract anyway), (b) the standalone `libperry_stdlib.a`, and (c)
    // the standalone `libperry_runtime.a`. Drop a staticlib member only
    // if **every** symbol it defines is also in that union — meaning the
    // linker can resolve every reference to those symbols from one of
    // the other inputs. Members with even one unique symbol (typical
    // for crate-specific generic monomorphizations) are kept.
    //
    // The previous code dropped by name-pattern (`perry_runtime-` /
    // `perry_stdlib-` member name prefix), which silently stripped
    // unique CGUs and broke Linux builds (#181 part B, v0.5.320). The
    // fragile UI-crate-prefix dedup that compared the staticlib member
    // name to the first rlib object's name prefix is also gone — the
    // rlib's symbols are now part of the provided set, so any member
    // whose contents are fully duplicated by the rlib gets dropped on
    // evidence rather than naming convention.
    //
    // Falls back to the legacy `.dll` / `compiler_builtins` short-circuits
    // plus the rlib name-prefix check when llvm-nm isn't available.
    let llvm_nm = find_nightly_llvm_tool("llvm-nm")
        .or_else(|| find_llvm_tool("llvm-nm"))
        .or_else(|| find_path_tool("nm"));
    let nm_works = llvm_nm.as_ref().is_some_and(|nm| {
        // Probe with a trivial call; if it can't even run, skip the
        // symbol-set path entirely.
        Command::new(nm)
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
    });

    // Build provided-symbols union when nm is available.
    let provided_symbols: std::collections::HashSet<String> = if nm_works {
        let nm = llvm_nm.as_ref().expect("nm_works ⇒ Some");
        let mut syms: std::collections::HashSet<String> = std::collections::HashSet::new();
        if has_rlib {
            let abs_rlib = std::fs::canonicalize(&rlib_path).unwrap_or_else(|_| rlib_path.clone());
            let n = syms.len();
            syms.extend(collect_archive_symbols_flat(nm, &abs_rlib));
            eprintln!("[strip-dedup] rlib symbols loaded: {}", syms.len() - n);
        }
        if let Some(ref sp) = stdlib_path {
            let abs = std::fs::canonicalize(sp).unwrap_or_else(|_| sp.clone());
            let n = syms.len();
            syms.extend(collect_archive_symbols_flat(nm, &abs));
            eprintln!(
                "[strip-dedup] {stdlib_name} symbols loaded: {}",
                syms.len() - n
            );
        }
        if let Some(ref rp) = runtime_path {
            let abs = std::fs::canonicalize(rp).unwrap_or_else(|_| rp.clone());
            let n = syms.len();
            syms.extend(collect_archive_symbols_flat(nm, &abs));
            eprintln!(
                "[strip-dedup] {runtime_name} symbols loaded: {}",
                syms.len() - n
            );
        }
        syms
    } else {
        eprintln!("[strip-dedup] llvm-nm unavailable — falling back to name-pattern dedup");
        std::collections::HashSet::new()
    };

    // Per-member symbols of the bundling staticlib (lazy-init to skip the
    // whole nm parse if nm isn't usable).
    let staticlib_member_symbols = if nm_works {
        let nm = llvm_nm.as_ref().expect("nm_works ⇒ Some");
        collect_archive_symbols_by_member(nm, &abs_staticlib).unwrap_or_default()
    } else {
        std::collections::HashMap::new()
    };

    let mut excluded_by_subset = 0usize;
    let mut excluded_by_pattern = 0usize;
    let ui_only_deps: Vec<&String> = staticlib_members
        .iter()
        .filter(|m| {
            if m.ends_with(".dll") {
                return false;
            }
            if m.contains("compiler_builtins") {
                excluded_by_pattern += 1;
                return false;
            }

            // Symbol-set path: drop only if every defined symbol is also
            // provided elsewhere. Members with no defined symbols (e.g.
            // marker TUs, inline-only headers) are kept defensively.
            if nm_works {
                if let Some(member_syms) = staticlib_member_symbols.get(m.as_str()) {
                    if !member_syms.is_empty()
                        && member_syms.iter().all(|s| provided_symbols.contains(s))
                    {
                        excluded_by_subset += 1;
                        return false;
                    }
                }
                // Member not found in nm output → keep (defensive — could be
                // a Mach-O archive nm version skew).
                return true;
            }

            // Fallback: legacy name-pattern when nm is unavailable. The
            // `exclude_members` set is from `ar t` member names (recorded
            // for diagnostics). We don't actually drop on this in the new
            // logic because name collisions between archives don't imply
            // symbol overlap (#181 Arch Linux), but on the no-nm fallback
            // we restore the rlib-prefix shortcut so the UI crate's own
            // CGUs aren't double-included.
            if exclude_members.contains(m.as_str()) {
                // Counted only — not excluded. Same reasoning as #181.
            }
            if has_rlib {
                if let Some(prefix) = rlib_objects
                    .first()
                    .and_then(|o| o.split('.').next())
                    .and_then(|s| s.split('-').next())
                {
                    if m.starts_with(&format!("{}-", prefix)) {
                        excluded_by_pattern += 1;
                        return false;
                    }
                }
            }
            true
        })
        .collect();

    eprintln!("[strip-dedup] {lib_name}: keeping {} of {} members (excluded: {} by symbol-subset, {} by name pattern)",
        ui_only_deps.len(), staticlib_members.len(), excluded_by_subset, excluded_by_pattern);

    // Write trimmed lib to a temp directory — the source lib may be on a read-only mount (e.g. Docker)
    let tmp_base = std::env::temp_dir().join(format!("perry_strip_{}", std::process::id()));
    std::fs::create_dir_all(&tmp_base).ok();
    let trimmed_lib = tmp_base.join(format!("_{lib_name}_trimmed.lib"));
    let extract_dir = tmp_base.join(format!("_{lib_name}_extract"));
    let _ = std::fs::remove_dir_all(&extract_dir);
    std::fs::create_dir_all(&extract_dir)?;

    let mut all_objects: Vec<std::path::PathBuf> = Vec::new();

    // If we have an rlib, extract UI crate objects from it (skipping alloc shims).
    if has_rlib {
        let abs_rlib = std::fs::canonicalize(&rlib_path)?;
        let mut rlib_extracted = 0usize;
        let mut rlib_skipped = 0usize;
        for member in &rlib_objects {
            let is_alloc_shim = !member.contains(".cgu.") && !member.contains("-cgu.");
            if is_alloc_shim {
                rlib_skipped += 1;
                continue;
            }
            let out = Command::new(&llvm_ar)
                .arg("x")
                .arg(&abs_rlib)
                .arg(member)
                .current_dir(&extract_dir)
                .output()?;
            if out.status.success() {
                let p = extract_dir.join(member);
                if p.exists() {
                    all_objects.push(p);
                    rlib_extracted += 1;
                }
            }
        }
        eprintln!(
            "[strip-dedup] rlib: extracted {rlib_extracted}, skipped {rlib_skipped} alloc shims"
        );
    }

    // Extract UI-only deps from staticlib. #854: only `extract_fail`
    // is read (the warning below); the parallel `extract_ok` counter
    // was incremented but never reported. Dropped.
    let mut extract_fail = 0usize;
    for member in &ui_only_deps {
        let out = Command::new(&llvm_ar)
            .arg("x")
            .arg(&abs_staticlib)
            .arg(member.as_str())
            .current_dir(&extract_dir)
            .output()?;
        if out.status.success() {
            let p = extract_dir.join(member.as_str());
            if p.exists() {
                all_objects.push(p);
            }
        } else {
            extract_fail += 1;
        }
    }
    if extract_fail > 0 {
        eprintln!("[strip-dedup] WARNING: {extract_fail} members failed to extract from staticlib");
    }

    eprintln!(
        "[strip-dedup] Building trimmed {lib_name}: {} objects total",
        all_objects.len()
    );

    // Create new archive from just the UI-specific objects
    let mut ar_cmd = Command::new(&llvm_ar);
    ar_cmd.arg("crs").arg(&trimmed_lib);
    for p in &all_objects {
        ar_cmd.arg(p);
    }
    let ar_out = ar_cmd.output()?;
    if !ar_out.status.success() {
        let stderr = String::from_utf8_lossy(&ar_out.stderr);
        eprintln!("[strip-dedup] ERROR: archive creation failed: {}", stderr);
        let _ = std::fs::remove_dir_all(&extract_dir);
        return Err(anyhow::anyhow!(
            "Failed to create trimmed archive for {lib_name}: {stderr}"
        ));
    }

    eprintln!(
        "[strip-dedup] OK: {} -> {}",
        lib_path.display(),
        trimmed_lib.display()
    );
    let _ = std::fs::remove_dir_all(&extract_dir);
    let _ = std::fs::remove_dir_all("_perry_ui_objects");
    Ok(trimmed_lib)
}

pub(super) fn strip_duplicate_objects_from_well_known_lib(lib_path: &PathBuf) -> Result<PathBuf> {
    let lib_name = lib_path.file_name().and_then(|f| f.to_str()).unwrap_or("?");
    eprintln!(
        "[strip-dedup] Processing well-known wrapper: {}",
        lib_path.display()
    );

    let llvm_ar = find_llvm_tool("llvm-ar")
        .or_else(|| find_path_tool("ar"))
        .ok_or_else(|| anyhow::anyhow!("ar not found"))?;
    let objcopy = find_nightly_llvm_tool("llvm-objcopy")
        .or_else(|| find_llvm_tool("llvm-objcopy"))
        .or_else(|| find_path_tool("objcopy"))
        .ok_or_else(|| anyhow::anyhow!("objcopy not found"))?;
    let nm = find_nightly_llvm_tool("llvm-nm")
        .or_else(|| find_llvm_tool("llvm-nm"))
        .or_else(|| find_path_tool("nm"))
        .ok_or_else(|| anyhow::anyhow!("nm not found"))?;

    let abs_staticlib = std::fs::canonicalize(lib_path)?;
    let symbols_by_member = collect_archive_symbols_by_member(&nm, &abs_staticlib)
        .ok_or_else(|| anyhow::anyhow!("failed to inspect archive symbols"))?;
    let forced_symbols_by_member: std::collections::BTreeMap<String, Vec<String>> =
        symbols_by_member
            .iter()
            .filter_map(|(member, symbols)| {
                let mut forced_symbols: Vec<String> = symbols
                    .iter()
                    .filter(|symbol| force_localize_symbol(symbol))
                    .cloned()
                    .collect();
                if forced_symbols.is_empty() {
                    None
                } else {
                    forced_symbols.sort();
                    Some((member.clone(), forced_symbols))
                }
            })
            .collect();
    if forced_symbols_by_member.is_empty() {
        return Ok(lib_path.clone());
    }

    let members_out = Command::new(&llvm_ar)
        .arg("t")
        .arg(&abs_staticlib)
        .output()?;
    if !members_out.status.success() {
        return Err(anyhow::anyhow!("failed to list archive members"));
    }
    let members: Vec<String> = String::from_utf8_lossy(&members_out.stdout)
        .lines()
        .map(|line| line.to_string())
        .collect();

    let tmp_base = std::env::temp_dir().join(format!("perry_strip_{}", std::process::id()));
    std::fs::create_dir_all(&tmp_base).ok();
    let extract_dir = tmp_base.join(format!("_{lib_name}_well_known_extract"));
    let _ = std::fs::remove_dir_all(&extract_dir);
    std::fs::create_dir_all(&extract_dir)?;
    let trimmed_lib = tmp_base.join(format!("_{lib_name}_trimmed.lib"));

    let extract_out = Command::new(&llvm_ar)
        .arg("x")
        .arg(&abs_staticlib)
        .current_dir(&extract_dir)
        .output()?;
    if !extract_out.status.success() {
        let stderr = String::from_utf8_lossy(&extract_out.stderr);
        return Err(anyhow::anyhow!("failed to extract {lib_name}: {stderr}"));
    }

    for (member, symbols) in &forced_symbols_by_member {
        let member_path = extract_dir.join(member);
        // On ELF, localizing the panic/unwind personality symbols (including the
        // compiler-emitted `DW.ref.rust_eh_personality`) breaks PIE relocations
        // → "undefined hidden symbol ... can not be used when making a PIE
        // object". That dedup is only needed for tier-3 Mach-O (-Zbuild-std);
        // skip it for ELF members and keep localizing the allocator shims.
        let skip_panic_unwind = object_is_elf(&member_path);
        for symbol in symbols {
            if skip_panic_unwind && is_panic_unwind_symbol(symbol) {
                continue;
            }
            let out = Command::new(&objcopy)
                .arg("--localize-symbol")
                .arg(symbol)
                .arg(&member_path)
                .output()?;
            if !out.status.success() {
                let stderr = String::from_utf8_lossy(&out.stderr);
                return Err(anyhow::anyhow!(
                    "failed to localize {symbol} in {member}: {stderr}"
                ));
            }
        }
    }

    let mut ar_cmd = Command::new(&llvm_ar);
    ar_cmd.arg("crs").arg(&trimmed_lib);
    for member in &members {
        ar_cmd.arg(extract_dir.join(member));
    }
    let ar_out = ar_cmd.output()?;
    if !ar_out.status.success() {
        let stderr = String::from_utf8_lossy(&ar_out.stderr);
        return Err(anyhow::anyhow!(
            "failed to create well-known wrapper archive for {lib_name}: {stderr}"
        ));
    }

    eprintln!(
        "[strip-dedup] {lib_name}: localized wrapper-only globals in {} member(s)",
        forced_symbols_by_member.len()
    );
    let _ = std::fs::remove_dir_all(&extract_dir);
    Ok(trimmed_lib)
}

/// Drop a well-known wrapper's bundled `perry_runtime-*` codegen unit(s) when
/// the perry-stdlib archive that follows on the link line bundles the same
/// unit.
///
/// Wrapper staticlibs (perry-ext-http, …) bundle their whole Rust dep graph,
/// including a full copy of perry-runtime. In the wrappers-BEFORE-stdlib link
/// shapes (`prefer_well_known_before_stdlib`: out-of-tree prebuilt stdlib and
/// the auto-optimize archives-fresh fast path), that bundled copy becomes the
/// first-definition winner for every extern runtime symbol the user object
/// references (`js_wait_for_event`, `js_promise_run_microtasks`, …). Meanwhile
/// perry-stdlib's own code keeps using ITS bundled runtime copy through
/// LTO-promoted internal references (`.llvm.`-suffixed names resolve only
/// intra-archive). The process then runs TWO disjoint copies of the runtime's
/// mutable globals — two event-pump wait-driver slots, two microtask queues,
/// two exception states. Concretely: an async task spawned by stdlib code
/// (fetch) registers its wait-driver in stdlib's copy, the main loop's
/// `js_wait_for_event` — resolved from the wrapper's copy — reads a
/// never-written slot, falls back to the condvar park, and every spawned task
/// starves forever.
///
/// Decision rule (evidence-based, per the v0.5.331 dedup standard — see
/// [`strip_duplicate_objects_from_lib`]): a `perry_runtime-*` member is
/// dropped only when BOTH hold:
///  1. the stdlib archive bundles the same codegen unit — matched by member
///     name containment, since stdlib's packaging renames members to
///     `perry_stdlib-<hash>.<original-member-name>.rcgu.o` (same crate + cgu
///     hash ⇒ same rlib input, identical extern surface);
///  2. every symbol it defines that a *sibling* member references is also
///     defined by the stdlib archive (a sibling referencing one of the copy's
///     LTO-promoted `.llvm.` internals would go undefined — keep the member).
/// Anything the user object needs beyond stdlib's copy is provided by the
/// standalone `libperry_runtime.a` gap-filler linked after stdlib (the
/// long-standing DCE-fallback contract in `build_and_run_link`).
///
/// Non-fatal by construction: any nm/ar failure or rule miss returns the
/// original archive unchanged.
pub(super) fn strip_bundled_runtime_from_well_known_lib(
    lib_path: &PathBuf,
    stdlib_lib: &Path,
) -> Result<PathBuf> {
    let lib_name = lib_path.file_name().and_then(|f| f.to_str()).unwrap_or("?");

    let llvm_ar = find_llvm_tool("llvm-ar")
        .or_else(|| find_path_tool("ar"))
        .ok_or_else(|| anyhow::anyhow!("ar not found"))?;
    let nm = find_nightly_llvm_tool("llvm-nm")
        .or_else(|| find_llvm_tool("llvm-nm"))
        .or_else(|| find_path_tool("nm"))
        .ok_or_else(|| anyhow::anyhow!("nm not found"))?;

    let abs_lib = std::fs::canonicalize(lib_path)?;
    let abs_stdlib = std::fs::canonicalize(stdlib_lib)?;

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

    let members = list_members(&abs_lib)?;
    let candidates: Vec<String> = members
        .iter()
        .filter(|m| m.starts_with("perry_runtime-"))
        .cloned()
        .collect();
    if candidates.is_empty() {
        return Ok(lib_path.clone());
    }

    // Rule 1: stdlib must bundle the same codegen unit (renamed member
    // contains the original member name verbatim).
    let stdlib_members = list_members(&abs_stdlib)?;
    let candidates: Vec<String> = candidates
        .into_iter()
        .filter(|c| stdlib_members.iter().any(|s| s.contains(c.as_str())))
        .collect();
    if candidates.is_empty() {
        return Ok(lib_path.clone());
    }

    // Rule 2: no sibling member may depend on a symbol only this copy defines.
    let defined_by_member = collect_archive_symbols_by_member(&nm, &abs_lib)
        .ok_or_else(|| anyhow::anyhow!("failed to inspect defined symbols of {lib_name}"))?;
    let undefined_by_member = collect_archive_undefined_by_member(&nm, &abs_lib)
        .ok_or_else(|| anyhow::anyhow!("failed to inspect undefined symbols of {lib_name}"))?;
    let stdlib_defined = collect_archive_symbols_flat(&nm, &abs_stdlib);
    if stdlib_defined.is_empty() {
        return Err(anyhow::anyhow!(
            "failed to inspect stdlib symbols (empty set)"
        ));
    }
    let candidate_set: std::collections::BTreeSet<&String> = candidates.iter().collect();
    let sibling_undefined: std::collections::HashSet<&String> = undefined_by_member
        .iter()
        .filter(|(m, _)| !candidate_set.contains(m))
        .flat_map(|(_, syms)| syms.iter())
        .collect();
    let empty = std::collections::HashSet::new();
    let removable: Vec<&String> = candidates
        .iter()
        .filter(|c| {
            let defined = defined_by_member.get(*c).unwrap_or(&empty);
            let unsatisfied: Vec<&&String> = sibling_undefined
                .iter()
                .filter(|s| defined.contains(**s) && !stdlib_defined.contains(**s))
                .collect();
            if !unsatisfied.is_empty() {
                eprintln!(
                    "[strip-dedup] {lib_name}: keeping bundled {c} — {} sibling-referenced \
                     symbol(s) not provided by stdlib (e.g. {})",
                    unsatisfied.len(),
                    unsatisfied[0]
                );
            }
            unsatisfied.is_empty()
        })
        .collect();
    if removable.is_empty() {
        return Ok(lib_path.clone());
    }

    let tmp_base = std::env::temp_dir().join(format!("perry_strip_{}", std::process::id()));
    std::fs::create_dir_all(&tmp_base).ok();
    let extract_dir = tmp_base.join(format!("_{lib_name}_noruntime_extract"));
    let _ = std::fs::remove_dir_all(&extract_dir);
    std::fs::create_dir_all(&extract_dir)?;
    let trimmed_lib = tmp_base.join(format!("_{lib_name}_noruntime.lib"));
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

    let remove_set: std::collections::BTreeSet<&String> = removable.iter().copied().collect();
    let mut ar_cmd = Command::new(&llvm_ar);
    ar_cmd.arg("crs").arg(&trimmed_lib);
    for member in &members {
        if remove_set.contains(member) {
            continue;
        }
        ar_cmd.arg(extract_dir.join(member));
    }
    let ar_out = ar_cmd.output()?;
    if !ar_out.status.success() {
        let stderr = String::from_utf8_lossy(&ar_out.stderr);
        return Err(anyhow::anyhow!(
            "failed to create runtime-stripped archive for {lib_name}: {stderr}"
        ));
    }

    eprintln!(
        "[strip-dedup] {lib_name}: dropped {} bundled perry-runtime member(s) \
         (stdlib provides the single runtime copy)",
        remove_set.len()
    );
    let _ = std::fs::remove_dir_all(&extract_dir);
    Ok(trimmed_lib)
}

/// Mach-O companion to the well-known dropper for the prebuilt UI staticlib.
/// The UI lib ships from the release bundle, so every dependency copy it
/// bundles — perry-runtime, std, itoa, data-encoding, … — comes from a
/// foreign crate graph: member names can never match the (auto-optimized,
/// locally rebuilt) stdlib's bundled copies, Rule 1 of
/// [`strip_bundled_runtime_from_well_known_lib`] can't fire, and on Mach-O —
/// where ld64.lld has no `--allow-multiple-definition` — both copies load and
/// the link dies with duplicate `_js_*` / std / itoa externs.
///
/// The bundled members cannot simply be dropped: sibling UI members reach
/// shared-generic monomorphizations instantiated inside them
/// (`RawVec::grow_one`, `hashbrown::…::reserve_rehash`) whose symbol hashes
/// embed the foreign crate fingerprint — no locally rebuilt archive can ever
/// provide those. Instead, LOCALIZE every global a member defines that the
/// actually-linked stdlib/runtime archives also export: sibling references
/// rebind to the single linked copy (one set of runtime/std mutable state —
/// the #5920 invariant), each member keeps exporting only its unique
/// generics, and whatever becomes unreferenced dead-strips
/// (SUBSECTIONS_VIA_SYMBOLS). No global remains defined on both sides, so
/// the duplicate-symbol errors are structurally gone.
///
/// Non-fatal by construction: any nm/ar/objcopy failure returns the original
/// archive unchanged at the callsite.
pub(super) fn dedup_ui_lib_against_linked_libs(
    lib_path: &PathBuf,
    reference_libs: &[&Path],
) -> Result<PathBuf> {
    let lib_name = lib_path.file_name().and_then(|f| f.to_str()).unwrap_or("?");

    let llvm_ar = find_llvm_tool("llvm-ar")
        .or_else(|| find_path_tool("ar"))
        .ok_or_else(|| anyhow::anyhow!("ar not found"))?;
    let objcopy = find_nightly_llvm_tool("llvm-objcopy")
        .or_else(|| find_llvm_tool("llvm-objcopy"))
        .or_else(|| find_path_tool("objcopy"))
        .ok_or_else(|| anyhow::anyhow!("objcopy not found"))?;
    let nm = find_nightly_llvm_tool("llvm-nm")
        .or_else(|| find_llvm_tool("llvm-nm"))
        .or_else(|| find_path_tool("nm"))
        .ok_or_else(|| anyhow::anyhow!("nm not found"))?;

    let abs_lib = std::fs::canonicalize(lib_path)?;

    let out = Command::new(&llvm_ar).arg("t").arg(&abs_lib).output()?;
    if !out.status.success() {
        return Err(anyhow::anyhow!("failed to list members of {lib_name}"));
    }
    let members: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.to_string())
        .collect();

    // Union of the external symbols provided by the archives actually on the
    // link line (auto-optimized or prebuilt stdlib + the standalone runtime
    // gap-filler). Extern-only: a local definition over there cannot satisfy
    // a reference rebound away from the bundled copy.
    let mut linked_globals: std::collections::HashSet<String> = std::collections::HashSet::new();
    for reference in reference_libs {
        let abs_ref = std::fs::canonicalize(reference)?;
        linked_globals.extend(collect_archive_global_symbols_flat(&nm, &abs_ref));
    }
    if linked_globals.is_empty() {
        return Err(anyhow::anyhow!(
            "failed to inspect linked stdlib/runtime symbols (empty set)"
        ));
    }

    let defined_by_member = collect_archive_symbols_by_member(&nm, &abs_lib)
        .ok_or_else(|| anyhow::anyhow!("failed to inspect defined symbols of {lib_name}"))?;

    let mut to_localize_by_member: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    let mut total_localized = 0usize;
    for member in &members {
        let Some(defined) = defined_by_member.get(member) else {
            continue;
        };
        let mut duplicated: Vec<String> = defined
            .iter()
            .filter(|s| linked_globals.contains(*s))
            .cloned()
            .collect();
        if duplicated.is_empty() {
            continue;
        }
        duplicated.sort();
        total_localized += duplicated.len();
        to_localize_by_member.insert(member.clone(), duplicated);
    }
    if to_localize_by_member.is_empty() {
        return Ok(lib_path.clone());
    }
    eprintln!(
        "[strip-dedup] {lib_name}: localizing {total_localized} bundled global(s) across \
         {} member(s) already provided by the linked stdlib/runtime",
        to_localize_by_member.len()
    );

    let tmp_base = std::env::temp_dir().join(format!("perry_strip_{}", std::process::id()));
    std::fs::create_dir_all(&tmp_base).ok();
    let extract_dir = tmp_base.join(format!("_{lib_name}_uiruntime_extract"));
    let _ = std::fs::remove_dir_all(&extract_dir);
    std::fs::create_dir_all(&extract_dir)?;
    let trimmed_lib = tmp_base.join(format!("_{lib_name}_uiruntime.lib"));
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

    for (member, symbols) in &to_localize_by_member {
        let member_path = extract_dir.join(member);
        let list_path = extract_dir.join(format!("{member}.localize-list"));
        std::fs::write(&list_path, symbols.join("\n"))?;
        let out = Command::new(&objcopy)
            .arg(format!("--localize-symbols={}", list_path.display()))
            .arg(&member_path)
            .output()?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(anyhow::anyhow!(
                "failed to localize bundled runtime globals in {member}: {stderr}"
            ));
        }
        let _ = std::fs::remove_file(&list_path);
    }

    let mut ar_cmd = Command::new(&llvm_ar);
    ar_cmd.arg("crs").arg(&trimmed_lib);
    for member in &members {
        ar_cmd.arg(extract_dir.join(member));
    }
    let ar_out = ar_cmd.output()?;
    if !ar_out.status.success() {
        let stderr = String::from_utf8_lossy(&ar_out.stderr);
        return Err(anyhow::anyhow!(
            "failed to create runtime-localized archive for {lib_name}: {stderr}"
        ));
    }

    let _ = std::fs::remove_dir_all(&extract_dir);
    Ok(trimmed_lib)
}

/// Issue #5928 (companion to #5920/#5921): `strip_bundled_runtime_from_well_known_lib`
/// only targets `perry_runtime-*` codegen-unit members. When a program links
/// MULTIPLE well-known libraries that each independently bundle a full
/// "shared tokio" HTTP-client stack (e.g. both `http` and `fastify` need
/// tokio/hyper_util/h2/rustls/reqwest/ring), the SAME duplication shape
/// recurs for every shared transitive dependency those libraries have in
/// common with `libperry_stdlib.a` (which also bundles its own copies for
/// fetch/https/websocket support) — `std`/`core`/`alloc` themselves included.
/// macOS's current linker has no `-multiply_defined suppress` / `-ld_classic`
/// escape hatch anymore (verified obsolete on current toolchains), so these
/// surface as hard `ld: duplicate symbol` link failures rather than
/// first-definition-wins warnings.
///
/// This applies the SAME two safety rules as
/// `strip_bundled_runtime_from_well_known_lib` (stdlib bundles the identical
/// codegen unit; no OTHER kept member depends on a symbol only the
/// duplicate-candidate provides) to EVERY member, not just `perry_runtime-`
/// ones — a naive one-shot widening is NOT safe (candidates can depend on
/// EACH OTHER, e.g. `hyper_util`'s object referencing a symbol only
/// `tokio`'s object defines, both bundled in the same well-known lib and
/// both initially flagged as removable — removing both in one pass without
/// checking inter-candidate edges can silently drop something still
/// needed), so this is a FIXED-POINT iteration: each round recomputes
/// "undefined symbol references from every member NOT currently marked for
/// removal" against the SHRINKING kept-set, and protects (un-marks) any
/// still-marked candidate whose defined symbols are needed by that kept-set
/// and aren't covered by stdlib. Repeats until no candidate is newly
/// protected in a round. Verified safe against the `issue_5920_wrapper_
/// bundled_runtime_async_starvation` regression test (that test requires
/// `PERRY_LLVM_OBJCOPY`/`PERRY_LLVM_NM`/`PERRY_LLVM_AR` — or `llvm-objcopy`/
/// `llvm-nm`/`llvm-ar` on `PATH` — to actually exercise the strip-dedup
/// path at all; without them it silently no-ops and produces a much later,
/// confusing "N duplicate symbols" `ld` failure with no indication dedup
/// was skipped).
///
/// Reduces, but does not always fully eliminate, duplicate symbols for
/// programs needing several LARGE, deeply-interconnected well-known
/// libraries simultaneously (e.g. both `http` and `fastify`, each pulling
/// in the full reqwest/hyper_util/h2/rustls stack) — Rule 2 conservatively
/// protects more members as the dependency graph within a single archive
/// grows, since more of them turn out to be referenced by a sibling that
/// itself can't be removed. Fully eliminates duplicates for simpler
/// well-known libraries (e.g. `ioredis`, `net`, `ws`) whose bundled
/// dependency graphs are smaller.
pub(super) fn strip_bundled_shared_deps_from_well_known_lib(
    lib_path: &PathBuf,
    stdlib_lib: &Path,
) -> Result<PathBuf> {
    let lib_name = lib_path.file_name().and_then(|f| f.to_str()).unwrap_or("?");

    let llvm_ar = find_llvm_tool("llvm-ar")
        .or_else(|| find_path_tool("ar"))
        .ok_or_else(|| anyhow::anyhow!("ar not found"))?;
    let nm = find_nightly_llvm_tool("llvm-nm")
        .or_else(|| find_llvm_tool("llvm-nm"))
        .or_else(|| find_path_tool("nm"))
        .ok_or_else(|| anyhow::anyhow!("nm not found"))?;

    let abs_lib = std::fs::canonicalize(lib_path)?;
    let abs_stdlib = std::fs::canonicalize(stdlib_lib)?;

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

    let members = list_members(&abs_lib)?;

    // Rule 1: stdlib must bundle the identical codegen unit (its renamed
    // member contains this well-known lib's member name verbatim). No
    // crate-name restriction — the fixed-point loop below is what makes
    // considering every member safe.
    let stdlib_members = list_members(&abs_stdlib)?;
    let candidates: std::collections::BTreeSet<String> = members
        .iter()
        .filter(|m| stdlib_members.iter().any(|s| s.contains(m.as_str())))
        .cloned()
        .collect();
    if candidates.is_empty() {
        return Ok(lib_path.clone());
    }

    let defined_by_member = collect_archive_symbols_by_member(&nm, &abs_lib)
        .ok_or_else(|| anyhow::anyhow!("failed to inspect defined symbols of {lib_name}"))?;
    let undefined_by_member = collect_archive_undefined_by_member(&nm, &abs_lib)
        .ok_or_else(|| anyhow::anyhow!("failed to inspect undefined symbols of {lib_name}"))?;
    let stdlib_defined = collect_archive_symbols_flat(&nm, &abs_stdlib);
    if stdlib_defined.is_empty() {
        return Err(anyhow::anyhow!(
            "failed to inspect stdlib symbols (empty set)"
        ));
    }
    let empty = std::collections::HashSet::new();

    // Fixed-point loop: start by assuming every candidate is removable, then
    // repeatedly protect (un-mark) any candidate whose symbols are still
    // needed by the current kept-set (members - to_remove), until a round
    // protects nothing new.
    let mut to_remove: std::collections::BTreeSet<String> = candidates.clone();
    loop {
        let kept_undefined: std::collections::HashSet<&String> = undefined_by_member
            .iter()
            .filter(|(m, _)| !to_remove.contains(m.as_str()))
            .flat_map(|(_, syms)| syms.iter())
            .collect();
        let mut protected_this_round = false;
        for c in to_remove.clone().iter() {
            let defined = defined_by_member.get(c).unwrap_or(&empty);
            let still_needed = kept_undefined
                .iter()
                .any(|s| defined.contains(*s) && !stdlib_defined.contains(*s));
            if still_needed {
                to_remove.remove(c);
                protected_this_round = true;
            }
        }
        if !protected_this_round {
            break;
        }
    }
    if to_remove.is_empty() {
        return Ok(lib_path.clone());
    }
    for c in &candidates {
        if !to_remove.contains(c) {
            eprintln!(
                "[strip-dedup] {lib_name}: keeping bundled {c} — needed by a kept \
                 sibling and not provided by stdlib"
            );
        }
    }

    let tmp_base = std::env::temp_dir().join(format!("perry_strip_{}", std::process::id()));
    std::fs::create_dir_all(&tmp_base).ok();
    let extract_dir = tmp_base.join(format!("_{lib_name}_nosharedeps_extract"));
    let _ = std::fs::remove_dir_all(&extract_dir);
    std::fs::create_dir_all(&extract_dir)?;
    let trimmed_lib = tmp_base.join(format!("_{lib_name}_nosharedeps.lib"));
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
    for member in &members {
        if to_remove.contains(member) {
            continue;
        }
        ar_cmd.arg(extract_dir.join(member));
    }
    let ar_out = ar_cmd.output()?;
    if !ar_out.status.success() {
        let stderr = String::from_utf8_lossy(&ar_out.stderr);
        return Err(anyhow::anyhow!(
            "failed to create shared-deps-stripped archive for {lib_name}: {stderr}"
        ));
    }

    eprintln!(
        "[strip-dedup] {lib_name}: dropped {} bundled member(s) also provided by stdlib \
         (shared transitive deps, fixed-point safe)",
        to_remove.len()
    );
    let _ = std::fs::remove_dir_all(&extract_dir);
    Ok(trimmed_lib)
}

mod stub_symbols;
use stub_symbols::strip_members_present_in_reference;
pub(super) use stub_symbols::{
    localize_stdlib_stub_symbols, localize_stdlib_stub_symbols_for_windows,
};

/// Tier-3 (tvOS/watchOS, no prebuilt std): perry-stdlib is built with
/// `-Zbuild-std` and bundles its own copy of std's allocator/panic runtime
/// shims, which duplicate the ones in runtime_lib (the canonical provider) →
/// `ld64.lld: duplicate symbol`. Localize those shims in the stdlib copy.
/// No-op (clone) on every other target; a strip failure is non-fatal and
/// falls back to the original archive.
pub(super) fn dedup_stdlib_for_tier3(_target: Option<&str>, stdlib: &PathBuf) -> PathBuf {
    // perry-stdlib is kept WHOLE on tier-3 and is the authoritative provider of
    // std/core/alloc + the allocator/panic shims. It is earliest on the link
    // line, so its std symbols win first-definition and stop ld64 from pulling
    // the duplicate std objects out of perry-runtime and the native binding lib
    // (which would then collide on e.g. `__rdl_alloc`). The de-duplication for
    // tier-3 happens on the *other* archives instead: [`dedup_runtime_for_tier3`]
    // strips perry-runtime's copies of stdlib's objects, and
    // [`dedup_native_lib_for_tier3`] localizes the native lib's allocator shims.
    // (Localizing the allocator *here* would leave no global allocator once the
    // runtime copy is stripped, producing undefined-symbol errors.)
    stdlib.clone()
}

/// Tier-3 (tvOS/watchOS) dedup for perry-runtime against perry-stdlib.
///
/// The auto-optimizer rebuilds perry-stdlib and perry-runtime from the same
/// `-Zbuild-std` crate graph, so perry-stdlib bundles byte-identical copies of
/// perry-runtime's std/core/alloc/perry_runtime objects. perry-stdlib is linked
/// whole and first (see [`dedup_stdlib_for_tier3`]), so strip every member from
/// perry-runtime that perry-stdlib already provides — leaving only the
/// runtime-unique members (e.g. the ios-game-loop variant object) and exactly
/// one copy of each symbol for ld64. No-op (clone) off tier-3.
pub(super) fn dedup_runtime_for_tier3(
    target: Option<&str>,
    runtime: &Path,
    stdlib: &Path,
) -> PathBuf {
    if matches!(target, Some("tvos") | Some("watchos")) {
        match strip_members_present_in_reference(runtime, stdlib, "") {
            Ok(t) => t,
            Err(e) => {
                eprintln!("[strip-dedup] runtime-vs-stdlib dedup skipped (non-fatal): {e}");
                runtime.to_path_buf()
            }
        }
    } else {
        runtime.to_path_buf()
    }
}

/// Tier-3 Apple (tvOS/watchOS) dedup for a per-crate native binding staticlib.
/// Same `-Zbuild-std` std-duplication as [`dedup_stdlib_for_tier3`] (alloc/
/// panic/eh runtime: `__rust_drop_panic`, `__rdl_alloc`, …) colliding with
/// perry-runtime's std at the final link. Skips shared libs (`.so`, Android)
/// and every non-tier-3 target (ios/macos use prebuilt std and don't hit this).
/// A strip failure is non-fatal and falls back to the original lib.
pub(super) fn dedup_native_lib_for_tier3(
    target: Option<&str>,
    lib_name: &str,
    lib: PathBuf,
) -> PathBuf {
    if matches!(target, Some("tvos") | Some("watchos")) && !lib_name.ends_with(".so") {
        let trimmed = match strip_duplicate_objects_from_lib(&lib) {
            Ok(trimmed) => trimmed,
            Err(e) => {
                eprintln!("[strip-dedup] skipped for native lib {lib_name} (non-fatal): {e}");
                lib
            }
        };
        // The member-subset trim removes the native crate's std objects that are
        // a clean subset of perry-stdlib, but its allocator/panic/EH shim cgu
        // (`alloc-*.rcgu.o`) carries extra monomorphizations so it survives — and
        // its `__rdl_alloc` / `rust_eh_personality` / … globals then collide with
        // perry-stdlib's. Localize those shim symbols here so perry-stdlib stays
        // the single global allocator.
        match strip_duplicate_objects_from_well_known_lib(&trimmed) {
            Ok(localized) => localized,
            Err(e) => {
                eprintln!(
                    "[strip-dedup] allocator localize skipped for native lib {lib_name} (non-fatal): {e}"
                );
                trimmed
            }
        }
    } else {
        lib
    }
}

#[cfg(test)]
mod strip_dedup_tests {
    use super::{force_localize_symbol, is_panic_unwind_symbol, parse_nm_archive_output};

    #[test]
    fn panic_unwind_classification_matches_dwref() {
        // The compiler-emitted `DW.ref.rust_eh_personality` substring-matches
        // `rust_eh_personality`; both must be treated as panic/unwind so the
        // well-known localizer skips them on ELF (PIE relocation breakage).
        assert!(is_panic_unwind_symbol("rust_eh_personality"));
        assert!(is_panic_unwind_symbol("DW.ref.rust_eh_personality"));
        assert!(is_panic_unwind_symbol("rust_begin_unwind"));
        assert!(is_panic_unwind_symbol("rust_panic"));
        // Allocator shims and ordinary symbols are not panic/unwind.
        assert!(!is_panic_unwind_symbol("__rust_alloc"));
        assert!(!is_panic_unwind_symbol("__rdl_dealloc"));
        assert!(!is_panic_unwind_symbol("js_fetch_with_options"));

        // Candidate collection is unchanged: allocator shims and the
        // panic/unwind group are both still force-localize candidates (the ELF
        // skip happens per-object in the well-known localizer, not here).
        assert!(force_localize_symbol("rust_eh_personality"));
        assert!(force_localize_symbol("__rust_alloc"));
        assert!(force_localize_symbol("js_stdlib_init_dispatch"));
        assert!(!force_localize_symbol("js_some_regular_export"));
    }

    #[test]
    fn parser_handles_bare_member_headers() {
        let nm_out = "\
member_one.o:
_sym_a
_sym_b

member_two.o:
_sym_c
";
        let map = parse_nm_archive_output(nm_out);
        assert_eq!(map.len(), 2);
        assert!(map["member_one.o"].contains("_sym_a"));
        assert!(map["member_one.o"].contains("_sym_b"));
        assert_eq!(map["member_one.o"].len(), 2);
        assert_eq!(map["member_two.o"].len(), 1);
        assert!(map["member_two.o"].contains("_sym_c"));
    }

    #[test]
    fn parser_strips_archive_wrapper_from_header() {
        // Some llvm-nm versions wrap each member as
        // `archive.a(member.o):` — we want the bare member name so the
        // map keys match `ar t` output.
        let nm_out = "\
/path/to/lib.a(perry_runtime-abc.cgu.0.rcgu.o):
_SYM
";
        let map = parse_nm_archive_output(nm_out);
        assert_eq!(map.len(), 1);
        assert!(map.contains_key("perry_runtime-abc.cgu.0.rcgu.o"));
    }

    #[test]
    fn parser_handles_bsd_symbol_lines() {
        let nm_out = "\
member_one.o:
0000000000000000 T _sym_a
0000000000000010 r .Lprivate

member_two.o:
0000000000000000 T js_stdlib_init_dispatch
";
        let map = parse_nm_archive_output(nm_out);
        assert_eq!(map.len(), 2);
        assert!(map["member_one.o"].contains("_sym_a"));
        assert!(map["member_one.o"].contains(".Lprivate"));
        assert!(map["member_two.o"].contains("js_stdlib_init_dispatch"));
    }

    #[test]
    fn parser_skips_empty_members() {
        let nm_out = "\
empty.o:

next.o:
_sym
";
        let map = parse_nm_archive_output(nm_out);
        // Empty.o produces no entry — `member_syms.is_empty()` is the
        // call-site guard that keeps zero-symbol members anyway.
        assert!(!map.contains_key("empty.o"));
        assert_eq!(map["next.o"].len(), 1);
    }

    #[test]
    fn subset_check_prunes_only_full_overlap() {
        // The actual filter logic: keep a member iff at least one of its
        // symbols is unique (i.e. not in the provided set). This pins
        // down the v0.5.320 #181 invariant — a member with a unique
        // generic monomorphization (not in standalone runtime/stdlib)
        // must be KEPT even if its name happens to match the pattern.
        let nm_out = "\
fully_dup.o:
_a
_b

unique_mono.o:
_a
_specific_to_this_lib

empty_marker.o:
";
        let by_member = parse_nm_archive_output(nm_out);
        let provided: std::collections::HashSet<String> =
            ["_a".to_string(), "_b".to_string(), "_z".to_string()]
                .into_iter()
                .collect();

        // fully_dup.o → all symbols provided → drop
        let m1 = &by_member["fully_dup.o"];
        assert!(!m1.is_empty() && m1.iter().all(|s| provided.contains(s)));

        // unique_mono.o → has _specific_to_this_lib not in provided → keep
        let m2 = &by_member["unique_mono.o"];
        assert!(!m2.is_empty() && !m2.iter().all(|s| provided.contains(s)));

        // empty_marker.o → no entry; call site keeps it defensively.
        assert!(!by_member.contains_key("empty_marker.o"));
    }
}
