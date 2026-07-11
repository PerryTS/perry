//! Auto-generate no-op `perry_ui_*` / `perry_system_*` / `perry_updater_*`
//! FFI stubs for the harmonyos build (#395 + #399), and the stub
//! manifest consumed by the runtime's first-call diagnostic + the
//! `perry check` static scan (#464).
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
//! We drive the stub list off `perry-dispatch`'s four tables —
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

use perry_dispatch::{ArgKind, MethodRow, ReturnKind};
use std::collections::HashSet;
use std::fmt::Write;

fn arg_kind_rust_type(k: ArgKind) -> &'static str {
    match k {
        ArgKind::Widget | ArgKind::I64Raw | ArgKind::Str => "i64",
        ArgKind::F64 | ArgKind::Closure => "f64",
    }
}

fn return_kind(k: ReturnKind) -> (Option<&'static str>, &'static str) {
    match k {
        ReturnKind::Widget | ReturnKind::Promise | ReturnKind::Str | ReturnKind::I64AsF64 => {
            (Some("i64"), "0")
        }
        ReturnKind::F64 => (Some("f64"), "0.0"),
        ReturnKind::Void => (None, ""),
    }
}

/// Per-table reason text + tracking issue. Surfaced in the stub
/// manifest and printed by `perry_stub_warn` on first call.
fn metadata_for(symbol: &str) -> (&'static str, Option<&'static str>) {
    if symbol.starts_with("perry_ui_") || symbol == "perry_get_device_idiom" {
        (
            "harmonyos perry/ui FFI: lowered code outside the ArkUI harvest target's App body has no real backing",
            Some("#395"),
        )
    } else if symbol.starts_with("perry_system_") {
        (
            "harmonyos perry/system FFI: not implemented on this build",
            Some("#399"),
        )
    } else if symbol.starts_with("perry_updater_") {
        (
            "harmonyos perry/updater FFI: not implemented on this build",
            Some("#399"),
        )
    } else {
        ("no-op stub on this build", None)
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

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../perry-dispatch/src/lib.rs");

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
    generate_single_byte_encodings(&out_dir);
    let stubs_dest = std::path::Path::new(&out_dir).join("perry_ui_harmonyos_stubs.rs");
    let manifest_dest = std::path::Path::new(&out_dir).join("perry_stub_manifest.rs");

    let mut stubs_out = String::new();
    stubs_out.push_str("// AUTO-GENERATED by perry-runtime/build.rs from perry-dispatch tables.\n");
    stubs_out.push_str("// Do not edit. See build.rs and crates/perry-dispatch/src/lib.rs.\n\n");

    let mut manifest_entries: Vec<String> = Vec::new();

    // The stub *function* file is only populated for the harmonyos
    // build — on every other target the platform UI crate
    // (perry-ui-macos / perry-ui-android / perry-ui-gtk4 /
    // perry-ui-windows / perry-ui-ios / etc.) owns the perry_ui_*
    // symbols and emitting them here would collide. The stub
    // *manifest* by contrast is always generated: `perry check` runs
    // on the host but needs to surface harmonyos stub warnings to
    // a developer cross-checking a harmonyos build, so the manifest
    // describes what *would* be stubbed for that target regardless
    // of the host's feature flags.
    let ohos_napi = std::env::var("CARGO_FEATURE_OHOS_NAPI").is_ok();
    if !ohos_napi {
        stubs_out.push_str(
            "// (No stub function bodies — `ohos-napi` feature not enabled.\n\
             //  Manifest is still populated for `perry check` consumption.)\n",
        );
    }

    let tables: &[&[MethodRow]] = &[
        perry_dispatch::PERRY_UI_TABLE,
        perry_dispatch::PERRY_UI_INSTANCE_TABLE,
        perry_dispatch::PERRY_SYSTEM_TABLE,
        perry_dispatch::PERRY_UPDATER_TABLE,
    ];
    let table_modules: &[&str] = &["perry/ui", "perry/ui", "perry/system", "perry/updater"];

    // Symbols owned by other modules in perry-runtime — stubbing here
    // would be a duplicate-symbol error at link time. Keep this list in
    // sync with `crates/perry-runtime/src/i18n.rs` +
    // `crates/perry-runtime/src/media_playback.rs`. The `perry_i18n_` /
    // `perry_media_` prefixes don't appear in the four tables above so
    // this is belt-and-braces — the actual filter is the table list.
    fn is_stubbable(runtime: &str) -> bool {
        runtime.starts_with("perry_ui_")
            || runtime.starts_with("perry_system_")
            || runtime.starts_with("perry_updater_")
            // `perry_get_device_idiom` is the one outlier in
            // PERRY_SYSTEM_TABLE that doesn't carry a `perry_system_`
            // prefix. Allow it explicitly.
            || runtime == "perry_get_device_idiom"
    }

    let mut seen: HashSet<&str> = HashSet::new();
    let mut count = 0usize;

    for (table, module) in tables.iter().zip(table_modules.iter()) {
        for row in table.iter() {
            if !is_stubbable(row.runtime) {
                continue;
            }
            // Some dispatch rows reuse the same runtime symbol (e.g.
            // `scrollViewSetChild` and `scrollviewSetChild` both →
            // `perry_ui_scrollview_set_child`). Emit each symbol once.
            if !seen.insert(row.runtime) {
                continue;
            }

            emit_stub(
                &mut stubs_out,
                row.runtime,
                row.args,
                row.ret,
                Some(row.method),
                module,
                &mut manifest_entries,
                ohos_napi,
            );
            count += 1;
        }
    }

    // Direct-call FFI symbols that bypass the dispatch tables — codegen
    // emits these for the simple constructor shapes (`VStack(items)`,
    // `HStack(items)`, `Button(label, onPress)`) in
    // `crates/perry-codegen/src/lower_call/native.rs`, and for the
    // trigger-shape-switched `notificationSchedule({trigger:{type,...}})`
    // in `crates/perry-codegen/src/lower_call.rs::lower_notification_schedule`.
    // They aren't in any PERRY_*_TABLE so the table walk above misses
    // them; we list them explicitly here. Keep in sync with the
    // hardcoded callsites — `grep -h '"perry_(ui|system|updater)_' \
    // crates/perry-codegen/src/**/*.rs | sort -u`.
    //
    // The `ts_name` column carries the user-facing TS function name when
    // it survives codegen as-is (`VStack`, `HStack`, `Button`). For the
    // notificationSchedule fan-out the TS-level name is the same
    // (`notificationSchedule`) for all three runtime variants — the
    // trigger-shape switch happens at lowering, not in source — so the
    // static scan dedupes on `ts_name` per module.
    let direct_call_stubs: &[(&str, &[ArgKind], ReturnKind, Option<&str>, &str)] = &[
        (
            "perry_ui_vstack_create",
            &[ArgKind::F64],
            ReturnKind::Widget,
            Some("VStack"),
            "perry/ui",
        ),
        (
            "perry_ui_hstack_create",
            &[ArgKind::F64],
            ReturnKind::Widget,
            Some("HStack"),
            "perry/ui",
        ),
        (
            "perry_ui_button_create",
            &[ArgKind::Str, ArgKind::Closure],
            ReturnKind::Widget,
            Some("Button"),
            "perry/ui",
        ),
        // notificationSchedule(...) — three trigger variants, dispatched
        // at compile time on `trigger.type`. Args are (id, title, body)
        // string ptrs as I64 then trigger-specific f64 fields. None of
        // these appear in PERRY_SYSTEM_TABLE (the table only carries the
        // simpler `notificationSend` shape).
        (
            "perry_system_notification_schedule_interval",
            &[
                ArgKind::Str,
                ArgKind::Str,
                ArgKind::Str,
                ArgKind::F64,
                ArgKind::F64,
            ],
            ReturnKind::Void,
            Some("notificationSchedule"),
            "perry/system",
        ),
        (
            "perry_system_notification_schedule_calendar",
            &[ArgKind::Str, ArgKind::Str, ArgKind::Str, ArgKind::F64],
            ReturnKind::Void,
            Some("notificationSchedule"),
            "perry/system",
        ),
        (
            "perry_system_notification_schedule_location",
            &[
                ArgKind::Str,
                ArgKind::Str,
                ArgKind::Str,
                ArgKind::F64,
                ArgKind::F64,
                ArgKind::F64,
            ],
            ReturnKind::Void,
            Some("notificationSchedule"),
            "perry/system",
        ),
    ];
    for (name, args, ret, ts_name, module) in direct_call_stubs {
        if !seen.insert(name) {
            continue;
        }
        emit_stub(
            &mut stubs_out,
            name,
            args,
            *ret,
            *ts_name,
            module,
            &mut manifest_entries,
            ohos_napi,
        );
        count += 1;
    }

    writeln!(stubs_out, "\n// {} stub(s) generated.", count).unwrap();
    std::fs::write(&stubs_dest, stubs_out).expect("failed to write stub file");

    let mut manifest_out = String::new();
    manifest_out
        .push_str("// AUTO-GENERATED by perry-runtime/build.rs from perry-dispatch tables.\n");
    manifest_out.push_str("// Do not edit. See build.rs.\n\n");
    manifest_out.push_str("pub const STUB_MANIFEST: &[StubEntry] = &[\n");
    for entry in &manifest_entries {
        manifest_out.push_str(entry);
    }
    manifest_out.push_str("];\n");
    std::fs::write(&manifest_dest, manifest_out).expect("failed to write manifest");
}

#[allow(clippy::too_many_arguments)]
fn emit_stub(
    out: &mut String,
    name: &str,
    args: &[ArgKind],
    ret: ReturnKind,
    ts_name: Option<&str>,
    module: &str,
    manifest_entries: &mut Vec<String>,
    emit_body: bool,
) {
    let (reason, issue) = metadata_for(name);

    if emit_body {
        let args_str: String = args
            .iter()
            .enumerate()
            .map(|(i, k)| format!("_a{}: {}", i, arg_kind_rust_type(*k)))
            .collect::<Vec<_>>()
            .join(", ");
        let (ret_ty, default) = return_kind(ret);

        let warn_call = format!(
            "    crate::stub_diag::perry_stub_warn({}, {}, {});\n",
            rust_str_lit(name),
            rust_str_lit(reason),
            rust_opt_str_lit(issue),
        );

        writeln!(out, "#[no_mangle]").unwrap();
        match ret_ty {
            Some(ty) => {
                writeln!(
                    out,
                    "pub extern \"C\" fn {}({}) -> {} {{",
                    name, args_str, ty
                )
                .unwrap();
                out.push_str(&warn_call);
                writeln!(out, "    {}", default).unwrap();
                writeln!(out, "}}").unwrap();
            }
            None => {
                writeln!(out, "pub extern \"C\" fn {}({}) {{", name, args_str).unwrap();
                out.push_str(&warn_call);
                writeln!(out, "}}").unwrap();
            }
        }
    }

    manifest_entries.push(format!(
        "    StubEntry {{ symbol: {}, ts_name: {}, module: {}, reason: {}, issue: {} }},\n",
        rust_str_lit(name),
        rust_opt_str_lit(ts_name),
        rust_str_lit(module),
        rust_str_lit(reason),
        rust_opt_str_lit(issue),
    ));
}
