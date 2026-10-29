//! Auto-generate no-op `perry_ui_*` / `perry_system_*` / `perry_updater_*`
//! FFI stubs for the harmonyos build (#395 + #399) from the stub list in
//! perry-runtime-manifest, which the `perry check` static scan (#464)
//! reads too.
//!
//! HarmonyOS goes through the `perry-codegen-arkts` harvest model — the
//! `App({body: VStack([...])})` literal is destructively rewritten into
//! ArkUI source so for the harvested widget tree the LLVM codegen never
//! sees `perry_ui_*` calls. There is no `perry-ui-harmonyos` crate by
//! design.
//!
//! But three families of `perry/*` FFI helpers leak into the lowered
//! `.so`:
//!
//! - **perry/ui** (#395): library factory functions like Hone's
//!   `createEditorPerryWidget`, event-handler closure bodies,
//!   conditional widget builders — anything not part of the harvest
//!   target's `App({body: ...})` literal.
//! - **perry/system** (#399): `isDarkMode()`, `getDeviceModel()`,
//!   `keychainSave/get`, `notificationSend`, etc. — never go through
//!   the harvest pass at all (the harvest only looks at the App body),
//!   so every `perry/system` call survives into LLVM codegen.
//! - **perry/updater** (#399): `install`, `verifyHash`,
//!   `compareVersions`, etc. — same shape as perry/system.
//!
//! Without these stubs the OHOS dynamic linker rejects the .so at app
//! launch with `Error relocating ... perry_X: symbol not found` and
//! the program never reaches `main`.
//!
//! The stub list is driven off `perry-dispatch`'s four tables —
//! PERRY_UI_TABLE + PERRY_UI_INSTANCE_TABLE + PERRY_SYSTEM_TABLE +
//! PERRY_UPDATER_TABLE — the same single-source-of-truth the LLVM
//! codegen consumes — so a new dispatch row automatically gets a stub
//! and there's no whack-a-mole.
//!
//! We deliberately SKIP the i18n + media tables: `perry_i18n_*` has
//! real implementations in `crates/perry-runtime/src/i18n.rs` (always
//! compiled), and `perry_media_*` has real implementations in
//! `crates/perry-runtime/src/media_playback.rs` (gated
//! `cfg(feature = "ohos-napi")`, harmonyos-only AVPlayer drain bridge).
//! Stubbing those would cause duplicate-symbol errors at link time.
//!
//! Each stub returns the zero-value for its declared `ReturnKind`
//! (handle 0 for Widget, 0.0 for F64, no-op for Void, null pointer for
//! Str). Before returning, the body funnels through
//! `crate::stub_diag::perry_stub_warn(symbol, reason, issue)` so the
//! first invocation per process per symbol prints a `[perry] warning`
//! line — see `src/stub_diag.rs` for the env-var policy.

use perry_dispatch::{ArgKind, ReturnKind};
use sha2::{Digest, Sha256};
use std::fmt::Write;
use std::path::{Path, PathBuf};

/// The source trees an archive is built from: the workspace manifest and lock
/// file, and every local crate perry-runtime builds with (its normal, optional
/// and build dependencies; `runtime_build_inputs_cover_the_runtime_closure` in
/// crates/perry checks this list against `cargo metadata`). Dirty or
/// source-only builds hash these, so a compiler built against one runtime
/// source rejects an archive built from another. Compiler-only trees are not
/// inputs: editing codegen must not rebuild and re-stamp identical archives.
const RUNTIME_BUILD_INPUTS: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "crates/perry-abi/Cargo.toml",
    "crates/perry-abi/src",
    "crates/perry-api-manifest/Cargo.toml",
    "crates/perry-api-manifest/src",
    "crates/perry-base64/Cargo.toml",
    "crates/perry-base64/src",
    "crates/perry-diagnostics/Cargo.toml",
    "crates/perry-diagnostics/src",
    "crates/perry-dispatch/Cargo.toml",
    "crates/perry-dispatch/src",
    "crates/perry-hex/Cargo.toml",
    "crates/perry-hex/src",
    "crates/perry-parser/Cargo.toml",
    "crates/perry-parser/src",
    "crates/perry-runtime-manifest/Cargo.toml",
    "crates/perry-runtime-manifest/src",
    "crates/perry-runtime/Cargo.toml",
    "crates/perry-runtime/build.rs",
    "crates/perry-runtime/src",
];

/// `cargo package` builds the crate from an isolated directory without the
/// workspace siblings above. Hash the packaged runtime itself in that layout;
/// the compiler and static wrapper both consume this same crate artifact.
const PACKAGED_RUNTIME_BUILD_INPUTS: &[&str] = &["Cargo.toml", "build.rs", "src"];

fn sanitize_build_id(value: &str) -> String {
    value
        .chars()
        .take(128)
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | ':') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn collect_source_files(root: &Path, path: &Path, out: &mut Vec<PathBuf>) {
    let Ok(metadata) = std::fs::metadata(path) else {
        return;
    };
    if metadata.is_file() {
        out.push(path.to_path_buf());
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let child = entry.path();
        if child.strip_prefix(root).ok().is_some_and(|relative| {
            relative
                .components()
                .any(|part| part.as_os_str() == "target" || part.as_os_str() == ".git")
        }) {
            continue;
        }
        collect_source_files(root, &child, out);
    }
}

fn source_build_id(root: &Path, inputs: &[&str]) -> String {
    let mut files = Vec::new();
    for relative in inputs {
        collect_source_files(root, &root.join(relative), &mut files);
    }
    files.sort();
    files.dedup();

    let mut hasher = Sha256::new();
    hasher.update(b"perry-runtime-build-inputs-v1\0");
    for path in files {
        println!("cargo:rerun-if-changed={}", path.display());
        let relative = path.strip_prefix(root).unwrap_or(&path);
        hasher.update(relative.to_string_lossy().replace('\\', "/").as_bytes());
        hasher.update(b"\0");
        match std::fs::read(&path) {
            Ok(bytes) => {
                hasher.update((bytes.len() as u64).to_le_bytes());
                hasher.update(bytes);
            }
            Err(_) => hasher.update(b"unreadable"),
        }
        hasher.update(b"\0");
    }
    let mut hex = String::with_capacity(64);
    for byte in hasher.finalize() {
        write!(hex, "{byte:02x}").expect("write source fingerprint");
    }
    format!("src:{hex}")
}

fn emit_runtime_build_id() {
    println!("cargo:rerun-if-env-changed=PERRY_BUILD_COMMIT");
    let manifest_dir =
        PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set"));
    let workspace_candidate = manifest_dir.join("../..");
    let workspace_layout = workspace_candidate
        .join("crates/perry-runtime/Cargo.toml")
        .is_file();
    let (root, inputs) = if workspace_layout {
        (workspace_candidate, RUNTIME_BUILD_INPUTS)
    } else {
        (manifest_dir, PACKAGED_RUNTIME_BUILD_INPUTS)
    };
    let root = root.canonicalize().unwrap_or(root);

    let explicit = std::env::var("PERRY_BUILD_COMMIT")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(|value| format!("git:{}", sanitize_build_id(value.trim())));

    // The id names the source, not the commit: a commit, rebase or checkout
    // that leaves these inputs byte-identical keeps the archives fresh.
    // Release builds pass PERRY_BUILD_COMMIT.
    let source_id = source_build_id(&root, inputs);
    let build_id = explicit.unwrap_or(source_id);
    println!("cargo:rustc-env=PERRY_RUNTIME_BUILD_ID={build_id}");
}

fn arg_kind_rust_type(k: ArgKind) -> &'static str {
    match k {
        ArgKind::Widget | ArgKind::I64Raw | ArgKind::Str => "i64",
        ArgKind::F64 | ArgKind::Closure => "f64",
    }
}

fn return_kind(k: ReturnKind) -> (Option<&'static str>, &'static str) {
    match k {
        ReturnKind::Widget
        | ReturnKind::Promise
        | ReturnKind::Str
        | ReturnKind::I64AsF64
        | ReturnKind::I64AsBool => (Some("i64"), "0"),
        ReturnKind::F64 => (Some("f64"), "0.0"),
        ReturnKind::Void => (None, ""),
    }
}

fn rust_str_lit(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn rust_opt_str_lit(s: Option<&str>) -> String {
    match s {
        Some(t) => format!("Some({})", rust_str_lit(t)),
        None => "None".to_string(),
    }
}

/// Generate the WHATWG single-byte TextDecoder index tables + label resolver
/// from `encoding_rs` (build-time only — only the `[u16; 128]` arrays and the
/// match arms land in the runtime binary). Keeps the tables spec-accurate
/// without hand-transcribing ~28 × 128 code points.
fn generate_single_byte_encodings(out_dir: &str) {
    // (canonical `.encoding` name, &[every WHATWG label]) — the single-byte
    // subset of encodings.json (mirrored by the WPT text-decoder label test).
    const SINGLE_BYTE: &[(&str, &[&str])] = &[
        ("ibm866", &["866", "cp866", "csibm866", "ibm866"]),
        (
            "iso-8859-2",
            &[
                "csisolatin2",
                "iso-8859-2",
                "iso-ir-101",
                "iso8859-2",
                "iso88592",
                "iso_8859-2",
                "iso_8859-2:1987",
                "l2",
                "latin2",
            ],
        ),
        (
            "iso-8859-3",
            &[
                "csisolatin3",
                "iso-8859-3",
                "iso-ir-109",
                "iso8859-3",
                "iso88593",
                "iso_8859-3",
                "iso_8859-3:1988",
                "l3",
                "latin3",
            ],
        ),
        (
            "iso-8859-4",
            &[
                "csisolatin4",
                "iso-8859-4",
                "iso-ir-110",
                "iso8859-4",
                "iso88594",
                "iso_8859-4",
                "iso_8859-4:1988",
                "l4",
                "latin4",
            ],
        ),
        (
            "iso-8859-5",
            &[
                "csisolatincyrillic",
                "cyrillic",
                "iso-8859-5",
                "iso-ir-144",
                "iso8859-5",
                "iso88595",
                "iso_8859-5",
                "iso_8859-5:1988",
            ],
        ),
        (
            "iso-8859-6",
            &[
                "arabic",
                "asmo-708",
                "csiso88596e",
                "csiso88596i",
                "csisolatinarabic",
                "ecma-114",
                "iso-8859-6",
                "iso-8859-6-e",
                "iso-8859-6-i",
                "iso-ir-127",
                "iso8859-6",
                "iso88596",
                "iso_8859-6",
                "iso_8859-6:1987",
            ],
        ),
        (
            "iso-8859-7",
            &[
                "csisolatingreek",
                "ecma-118",
                "elot_928",
                "greek",
                "greek8",
                "iso-8859-7",
                "iso-ir-126",
                "iso8859-7",
                "iso88597",
                "iso_8859-7",
                "iso_8859-7:1987",
                "sun_eu_greek",
            ],
        ),
        (
            "iso-8859-8",
            &[
                "csiso88598e",
                "csisolatinhebrew",
                "hebrew",
                "iso-8859-8",
                "iso-8859-8-e",
                "iso-ir-138",
                "iso8859-8",
                "iso88598",
                "iso_8859-8",
                "iso_8859-8:1988",
                "visual",
            ],
        ),
        ("iso-8859-8-i", &["csiso88598i", "iso-8859-8-i", "logical"]),
        (
            "iso-8859-10",
            &[
                "csisolatin6",
                "iso-8859-10",
                "iso-ir-157",
                "iso8859-10",
                "iso885910",
                "l6",
                "latin6",
            ],
        ),
        ("iso-8859-13", &["iso-8859-13", "iso8859-13", "iso885913"]),
        ("iso-8859-14", &["iso-8859-14", "iso8859-14", "iso885914"]),
        (
            "iso-8859-15",
            &[
                "csisolatin9",
                "iso-8859-15",
                "iso8859-15",
                "iso885915",
                "iso_8859-15",
                "l9",
            ],
        ),
        ("iso-8859-16", &["iso-8859-16"]),
        ("koi8-r", &["cskoi8r", "koi", "koi8", "koi8-r", "koi8_r"]),
        ("koi8-u", &["koi8-ru", "koi8-u"]),
        (
            "macintosh",
            &["csmacintosh", "mac", "macintosh", "x-mac-roman"],
        ),
        (
            "windows-874",
            &[
                "dos-874",
                "iso-8859-11",
                "iso8859-11",
                "iso885911",
                "tis-620",
                "windows-874",
            ],
        ),
        ("windows-1250", &["cp1250", "windows-1250", "x-cp1250"]),
        ("windows-1251", &["cp1251", "windows-1251", "x-cp1251"]),
        (
            "windows-1252",
            &[
                "ansi_x3.4-1968",
                "ascii",
                "cp1252",
                "cp819",
                "csisolatin1",
                "ibm819",
                "iso-8859-1",
                "iso-ir-100",
                "iso8859-1",
                "iso88591",
                "iso_8859-1",
                "iso_8859-1:1987",
                "l1",
                "latin1",
                "us-ascii",
                "windows-1252",
                "x-cp1252",
            ],
        ),
        ("windows-1253", &["cp1253", "windows-1253", "x-cp1253"]),
        (
            "windows-1254",
            &[
                "cp1254",
                "csisolatin5",
                "iso-8859-9",
                "iso-ir-148",
                "iso8859-9",
                "iso88599",
                "iso_8859-9",
                "iso_8859-9:1989",
                "l5",
                "latin5",
                "windows-1254",
                "x-cp1254",
            ],
        ),
        ("windows-1255", &["cp1255", "windows-1255", "x-cp1255"]),
        ("windows-1256", &["cp1256", "windows-1256", "x-cp1256"]),
        ("windows-1257", &["cp1257", "windows-1257", "x-cp1257"]),
        ("windows-1258", &["cp1258", "windows-1258", "x-cp1258"]),
        ("x-mac-cyrillic", &["x-mac-cyrillic", "x-mac-ukrainian"]),
        // Special single-byte: bytes 0x80..=0xFF map to U+F780..=U+F7FF (PUA).
        ("x-user-defined", &["x-user-defined"]),
    ];
    let mut out = String::from(
        "// @generated by perry-runtime/build.rs from encoding_rs. Do not edit.\n\
         // WHATWG single-byte TextDecoder index tables (high half, bytes 0x80..=0xFF).\n\n",
    );
    let mut resolver = String::from(
        "/// Map a trimmed+lowercased label to (high-half table, canonical `.encoding`).\n\
         pub(crate) fn resolve_single_byte(label: &str) -> Option<(&'static [u16; 128], &'static str)> {\n    match label {\n");
    for (canonical, labels) in SINGLE_BYTE {
        // iso-8859-8-i shares ISO-8859-8's decode table; encoding_rs collapses
        // the label, so fall back to the base for the table lookup.
        let table_label = if *canonical == "iso-8859-8-i" {
            "iso-8859-8"
        } else {
            canonical
        };
        let enc = encoding_rs::Encoding::for_label(table_label.as_bytes())
            .unwrap_or_else(|| panic!("encoding_rs: unknown single-byte encoding {canonical}"));
        let ident: String = canonical
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_uppercase()
                } else {
                    '_'
                }
            })
            .collect();
        let table_name = format!("SB_{ident}");
        out.push_str(&format!("static {table_name}: [u16; 128] = [\n    "));
        for b in 0x80u8..=0xFF {
            let buf = [b];
            let (cow, _) = enc.decode_without_bom_handling(&buf);
            let cp = cow.chars().next().map(|c| c as u32).unwrap_or(0xFFFD);
            assert!(cp <= 0xFFFF, "{canonical} byte {b:#x} -> non-BMP {cp:#x}");
            out.push_str(&format!("0x{cp:04X}, "));
            if b & 0x0F == 0x0F {
                out.push_str("\n    ");
            }
        }
        out.push_str("\n];\n");
        let pats: Vec<String> = labels.iter().map(|l| format!("{l:?}")).collect();
        resolver.push_str(&format!(
            "        {} => Some((&{}, {:?})),\n",
            pats.join(" | "),
            table_name,
            canonical
        ));
    }
    resolver.push_str("        _ => None,\n    }\n}\n");
    out.push_str(&resolver);
    std::fs::write(
        std::path::Path::new(out_dir).join("single_byte_encodings.rs"),
        out,
    )
    .expect("write single_byte_encodings.rs");
}

/// The GC/diagnostic instruments (`diagnostics`, `gc-instruments`, `hot-diag`:
/// census, from-space quarantine and scan, seeded schedule, verifiers,
/// telemetry and trace JSON, hot-path mutator probes) compile under the
/// `perry_diagnostics` / `perry_gc_instruments` / `perry_hot_diag` cfgs.
/// They are OFF in a release or production build and ON when
/// `PERRY_RUNTIME_INSTRUMENTS=1` (the instrumented prebuilt variant,
/// `scripts/build_instr_runtime.sh`) or when the build is a debug-profile one
/// (`cargo build` / `cargo test`, whose unit tests exercise them).
///
/// An env var rather than a feature: a feature can only be added by Cargo's
/// unification, and the ext crates co-built with the runtime (#6303/#7358)
/// enable `default`, which would switch the instruments back on; the build
/// script sees the variable in every one of those invocations alike. The
/// instrumented variant implies the contract-check cfgs, so its archives also
/// serve a `PERRY_CHECK_CONTRACTS=1` compile.
///
/// A binary built without the instruments and asked for an instrument knob at
/// run time refuses it at startup ("built without the GC instruments");
/// perry picks the instrumented archives itself when a knob is set while
/// compiling.
///
/// Returns whether this is the instrumented variant.
fn emit_instrument_cfgs() -> bool {
    println!("cargo:rerun-if-env-changed=PERRY_RUNTIME_INSTRUMENTS");
    let variant = std::env::var("PERRY_RUNTIME_INSTRUMENTS").is_ok_and(|v| v == "1");
    let debug_profile = std::env::var("PROFILE").is_ok_and(|p| p == "debug");
    let on = variant || debug_profile;
    for (feature_env, cfg) in [
        ("CARGO_FEATURE_DIAGNOSTICS", "perry_diagnostics"),
        ("CARGO_FEATURE_GC_INSTRUMENTS", "perry_gc_instruments"),
        ("CARGO_FEATURE_HOT_DIAG", "perry_hot_diag"),
    ] {
        println!("cargo:rustc-check-cfg=cfg({cfg})");
        if std::env::var_os(feature_env).is_some() && on {
            println!("cargo:rustc-cfg={cfg}");
        }
    }
    variant
}

/// Compile the runtime-catch shim `src/ffi/perry_trap.ll` (ehfree 2b) into
/// `libperry_trap.a`. It is built once, here, with the runtime; generated
/// programs never emit it.
///
/// LLVM IR needs clang. The cc crate's compiler is used when it is clang
/// (Apple, Android NDK: its target and deployment flags carry over). A GNU
/// `cc` is replaced by `clang` from PATH, and cargo-xwin's `clang-cl` by its
/// `clang` driver sibling, both with an explicit `--target`.
/// `PERRY_TRAP_CLANG` overrides the choice. On windows-msvc the custom
/// personality makes LLVM emit `.seh_handler perry_eh_personality, @unwind,
/// @except` plus the Itanium LSDA in `.xdata`, the shape generated code
/// already uses (eh_windows.rs).
fn build_trap_shim() {
    use std::ffi::OsString;
    use std::path::PathBuf;
    println!("cargo:rerun-if-env-changed=PERRY_TRAP_CLANG");
    let target = std::env::var("TARGET").expect("TARGET not set by Cargo");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR not set"));
    let tool = cc::Build::new().get_compiler();
    let explicit_target = || vec![OsString::from(format!("--target={target}"))];
    let (clang, mut args): (PathBuf, Vec<OsString>) =
        if let Some(clang) = std::env::var_os("PERRY_TRAP_CLANG") {
            (PathBuf::from(clang), explicit_target())
        } else if tool.is_like_clang() && !tool.is_like_msvc() {
            (tool.path().to_path_buf(), tool.args().to_vec())
        } else if tool.is_like_msvc() && tool.is_like_clang() {
            let sibling =
                tool.path()
                    .with_file_name(if cfg!(windows) { "clang.exe" } else { "clang" });
            let clang = if sibling.exists() {
                sibling
            } else {
                PathBuf::from("clang")
            };
            (clang, explicit_target())
        } else {
            (PathBuf::from("clang"), explicit_target())
        };
    let obj = out_dir.join("perry_trap.o");
    args.extend(
        [
            "-c",
            "-x",
            "ir",
            "-O2",
            "-Wno-override-module",
            "-Wno-unused-command-line-argument",
        ]
        .into_iter()
        .map(OsString::from),
    );
    args.push(OsString::from("src/ffi/perry_trap.ll"));
    args.push(OsString::from("-o"));
    args.push(obj.clone().into_os_string());
    let status = std::process::Command::new(&clang)
        .args(&args)
        .status()
        .unwrap_or_else(|e| panic!("perry_trap.ll needs clang ({}): {e}", clang.display()));
    assert!(
        status.success(),
        "compiling src/ffi/perry_trap.ll with {} failed ({status})",
        clang.display()
    );
    cc::Build::new().object(&obj).compile("perry_trap");
}

#[path = "build/realm_info_exports.rs"]
mod realm_info_exports;
#[path = "src/object/native_module/static_exports_build.rs"]
mod static_exports_build;

fn main() {
    static_exports_build::emit(Path::new(&std::env::var("OUT_DIR").unwrap()));
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let realm = perry_runtime_manifest::realm_source::generate(std::path::Path::new("src"), &out)
        .expect("emit runtime realm manifest");
    realm_info_exports::write(&realm, &out).expect("emit realm info export link test");

    println!("cargo:rerun-if-env-changed=PERRY_CHECK_CONTRACTS");
    println!("cargo:rustc-check-cfg=cfg(wasm_discard)");
    println!("cargo:rustc-check-cfg=cfg(perry_check_contracts)");
    println!("cargo:rustc-check-cfg=cfg(perry_contract_checks)");
    let instrumented_variant = emit_instrument_cfgs();
    if std::env::var("PERRY_CHECK_CONTRACTS").as_deref() == Ok("1") || instrumented_variant {
        println!("cargo:rustc-cfg=perry_check_contracts");
    }
    if std::env::var_os("CARGO_FEATURE_CONTRACT_CHECKS").is_some() || instrumented_variant {
        println!("cargo:rustc-cfg=perry_contract_checks");
    }
    // Keep page backing outside the Rust CGU. Its unreferenced object is not
    // extracted by ordinary links, preserving their code and literal layout.
    println!("cargo:rerun-if-changed=src/hot_text.c");
    cc::Build::new()
        .file("src/hot_text.c")
        .opt_level(3)
        .flag_if_supported("-ffunction-sections")
        .flag_if_supported("-fdata-sections")
        .compile("perry_hot_text");

    println!("cargo:rustc-check-cfg=cfg(perry_native_stack_maps)");
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let width = std::env::var("CARGO_CFG_TARGET_POINTER_WIDTH").unwrap_or_default();
    let vendor = std::env::var("CARGO_CFG_TARGET_VENDOR").unwrap_or_default();
    if width == "64"
        && matches!(arch.as_str(), "aarch64" | "x86_64")
        && (vendor == "apple" || os == "linux" || (os == "windows" && arch == "x86_64"))
    {
        println!("cargo:rustc-cfg=perry_native_stack_maps");
    }

    println!("cargo:rerun-if-changed=src/ffi/perry_memory_profile.c");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux")
        && std::env::var("CARGO_CFG_TARGET_POINTER_WIDTH").as_deref() == Ok("64")
        && std::env::var_os("CARGO_FEATURE_ALLOC_MIMALLOC").is_some()
    {
        let include = std::env::var_os("DEP_MIMALLOC_INCLUDE_DIR")
            .expect("alloc-mimalloc must expose its matching C headers");
        cc::Build::new()
            .file("src/ffi/perry_memory_profile.c")
            .include(include)
            .flag_if_supported("-std=c11")
            .compile("perry_memory_profile");
    }
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../perry-dispatch/src/lib.rs");
    println!("cargo:rerun-if-env-changed=TARGET");

    // Runtime catches (`exception::trap`). Native targets catch through the
    // landing-pad shim `perry_trap.ll`; WASI keeps the setjmp trampoline
    // (#9305: it must be compiled by a C compiler, rustc cannot express
    // `returns_twice`), which the wasm SjLj pass lowers to wasm EH.
    println!("cargo:rerun-if-changed=src/ffi/perry_trap.ll");
    // WASI uses the standardized wasm EH proposal for SjLj, paired with
    // wasi-libc's libsetjmp and the generated-code lowering (#11378).
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("wasi") {
        println!("cargo:rerun-if-changed=src/ffi/perry_wasi_net.c");
        println!("cargo:rerun-if-changed=src/ffi/perry_wasm_throw.c");
        println!("cargo:rerun-if-changed=src/ffi/perry_wasm_trap.ll");
        let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
        let trap = std::fs::read_to_string("src/ffi/perry_wasm_trap.ll")
            .unwrap()
            .replace(
                "CHECKPOINT_BYTES",
                &std::mem::size_of::<perry_abi::WasmEhCheckpoint>().to_string(),
            )
            .replace(
                "CHECKPOINT_ALIGN",
                &std::mem::align_of::<perry_abi::WasmEhCheckpoint>().to_string(),
            );
        let source = out.join("perry_wasm_trap.ll");
        std::fs::write(&source, trap).unwrap();
        let mut eh = cc::Build::new();
        eh.file(source)
            .file("src/ffi/perry_wasm_throw.c")
            .file("src/ffi/perry_wasi_net.c");
        // The `perry_net` imports live in their own archive member, which a
        // link loads only on request (`-u perry_net_host`). WASIp2 has sockets
        // of its own and gets no such member.
        println!("cargo:rerun-if-changed=src/ffi/perry_net_import.c");
        if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("p1") {
            eh.file("src/ffi/perry_net_import.c");
        }
        // The page-discard hook is the same shape: a weak fallback and, in a
        // member of its own, the `perry_host` import (`-u perry_wasm_discard_host`).
        println!("cargo:rerun-if-changed=src/ffi/perry_wasm_discard.c");
        println!("cargo:rerun-if-changed=src/ffi/perry_wasm_discard_import.c");
        if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("p1")
            && std::env::var_os("CARGO_FEATURE_WASM_DISCARD").is_some()
        {
            println!("cargo:rustc-cfg=wasm_discard");
            eh.file("src/ffi/perry_wasm_discard.c")
                .file("src/ffi/perry_wasm_discard_import.c");
        }
        eh.flag("-mexception-handling")
            .flag("-mllvm")
            .flag("-wasm-enable-eh")
            .flag("-mllvm")
            .flag("-wasm-use-legacy-eh=false")
            .compile("perry_wasm_eh");
    } else {
        build_trap_shim();
    }
    println!(
        "cargo:rustc-env=PERRY_RUNTIME_TARGET={}",
        std::env::var("TARGET").expect("TARGET not set by Cargo")
    );
    emit_runtime_build_id();

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
    let names = perry_runtime_manifest::node_api_host_symbols().collect::<Vec<_>>();
    let mut anchors = format!(
        "static NODE_API_HOST_SYMBOL_ANCHORS: [NodeApiSymbol; {}] = [\n",
        names.len()
    );
    for name in names {
        writeln!(anchors, "    NodeApiSymbol(super::{name} as *const ()),").unwrap();
    }
    anchors.push_str("];\n");
    std::fs::write(
        std::path::Path::new(&out_dir).join("node_api_host_symbol_anchors.rs"),
        anchors,
    )
    .expect("write Node-API symbol anchors");
    generate_single_byte_encodings(&out_dir);
    let stubs_dest = std::path::Path::new(&out_dir).join("perry_ui_harmonyos_stubs.rs");

    let mut stubs_out = String::new();
    stubs_out.push_str("// AUTO-GENERATED by perry-runtime/build.rs from perry-dispatch tables.\n");
    stubs_out.push_str("// Do not edit. See build.rs and crates/perry-dispatch/src/lib.rs.\n\n");

    // The stub *function* file is only populated for the harmonyos
    // build — on every other target the platform UI crate
    // (perry-ui-macos / perry-ui-android / perry-ui-gtk4 /
    // perry-ui-windows / perry-ui-ios / etc.) owns the perry_ui_*
    // symbols and emitting them here would collide. The stub list itself
    // lives in perry-runtime-manifest, where `perry check` reads it too.
    let ohos_napi = std::env::var("CARGO_FEATURE_OHOS_NAPI").is_ok();
    let stubs = perry_runtime_manifest::stubs();
    if ohos_napi {
        for stub in &stubs {
            emit_stub(&mut stubs_out, stub);
        }
    } else {
        stubs_out.push_str("// (No stub function bodies — `ohos-napi` feature not enabled.)\n");
    }
    writeln!(
        stubs_out,
        "\n// {} stub(s) generated.",
        if ohos_napi { stubs.len() } else { 0 }
    )
    .unwrap();
    std::fs::write(&stubs_dest, stubs_out).expect("failed to write stub file");
}

fn emit_stub(out: &mut String, stub: &perry_runtime_manifest::Stub) {
    let args_str: String = stub
        .args
        .iter()
        .enumerate()
        .map(|(i, k)| format!("_a{}: {}", i, arg_kind_rust_type(*k)))
        .collect::<Vec<_>>()
        .join(", ");
    let (ret_ty, default) = return_kind(stub.ret);

    let warn_call = format!(
        "    crate::stub_diag::perry_stub_warn({}, {}, {});\n",
        rust_str_lit(stub.symbol),
        rust_str_lit(stub.reason),
        rust_opt_str_lit(stub.issue),
    );

    writeln!(out, "#[no_mangle]").unwrap();
    match ret_ty {
        Some(ty) => {
            writeln!(
                out,
                "pub extern \"C\" fn {}({}) -> {} {{",
                stub.symbol, args_str, ty
            )
            .unwrap();
            out.push_str(&warn_call);
            writeln!(out, "    {}", default).unwrap();
            writeln!(out, "}}").unwrap();
        }
        None => {
            writeln!(out, "pub extern \"C\" fn {}({}) {{", stub.symbol, args_str).unwrap();
            out.push_str(&warn_call);
            writeln!(out, "}}").unwrap();
        }
    }
}
