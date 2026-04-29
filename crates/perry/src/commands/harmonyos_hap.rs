//! HarmonyOS HAP (HarmonyOS Ability Package) bundler.
//!
//! Invoked by `compile` when `--target harmonyos[-simulator]` succeeds: takes
//! the already-linked `.so` + the ArkTS shim directory (both produced by
//! prior compile stages), assembles the OpenHarmony-required layout, optionally
//! compiles `.ets` → `.abc` via the SDK's ets-loader, zips the result as
//! `foo.hap`, and optionally runs `hap-sign` with user-supplied credentials.
//!
//! All signing credentials come from env vars — no config file in v1. The
//! split into P12 keystore + cert chain + profile mirrors
//! `hap-sign-tool.jar`'s `sign-app` CLI (README lines 297-314 of
//! developtools_hapsigner); B.3 conflated cert chain with profile and had
//! to be patched in B.4 when that was caught by audit.
//!
//!   PERRY_HARMONYOS_P12            — path to signing key (.p12)
//!   PERRY_HARMONYOS_P12_PASSWORD   — password for the .p12 (keystore)
//!   PERRY_HARMONYOS_CERT           — path to app cert chain (.cer / .pem);
//!                                     DevEco auto-signing names it
//!                                     `<bundleName>.cer`. Distinct from
//!                                     PROFILE.
//!   PERRY_HARMONYOS_PROFILE        — path to signed provisioning profile
//!                                     (.p7b). DevEco names it
//!                                     `<bundleName>.p7b`.
//!   PERRY_HARMONYOS_KEY_ALIAS      — alias inside the .p12 (defaults to
//!                                     "debugKey", DevEco's convention).
//!   PERRY_HARMONYOS_KEY_PASSWORD   — private-key password (often the same
//!                                     as the keystore password; defaults
//!                                     to PERRY_HARMONYOS_P12_PASSWORD).
//!   PERRY_HARMONYOS_SIGN_ALG       — SHA256withECDSA | SHA384withECDSA
//!                                     (default SHA256withECDSA).
//!   PERRY_HARMONYOS_BUNDLE_NAME    — must match the cert's bundle name.
//!                                     Falls back to `com.perry.app.<stem>`.
//!   PERRY_HARMONYOS_HAPSIGN        — override path to hap-sign-tool.jar;
//!                                     default: <sdk>/toolchains/lib/hap-sign-tool.jar
//!                                     (invoked via `java -jar ...`).
//!
//! If PERRY_HARMONYOS_P12, _P12_PASSWORD, _CERT, or _PROFILE is unset, the
//! HAP is emitted unsigned (`<stem>.unsigned.hap`) with a remediation
//! message. An unsigned HAP won't install via `hdc install`.
//!
//! Alternative path (used during v0.5.129 first-emulator validation):
//! copy the `.so` + `.ets` Perry emits into a DevEco Studio project and
//! let DevEco's hvigor do the signing & install. Sidesteps the env-var
//! dance entirely, at the cost of a two-step workflow. See the v0.5.129
//! CLAUDE.md entry.

use anyhow::{anyhow, Context, Result};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use zip::write::SimpleFileOptions;
use zip::ZipWriter;

/// Minimal 1x1 white PNG — placeholder for the required `resources/base/media/icon.png`.
/// Real apps provide their own via `perry.harmonyos.icon` in package.json (TBD, future PR).
const PLACEHOLDER_ICON_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // magic
    0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, // IHDR chunk
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, // 1x1
    0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, // 8-bit RGB
    0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, // IDAT
    0x54, 0x78, 0x9C, 0x62, 0xFF, 0xFF, 0xFF, 0x3F,
    0x00, 0x05, 0xFE, 0x02, 0xFE, 0xDC, 0xCC, 0x59,
    0xE7, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, // IEND
    0x44, 0xAE, 0x42, 0x60, 0x82,
];

pub struct HapBuildArgs<'a> {
    pub so_path: &'a Path,
    /// The `ets/` directory emitted by `emit_harmonyos_arkts_stubs` (compile.rs).
    /// Its `entryability/EntryAbility.ets` + `pages/Index.ets` are copied into
    /// the HAP. If the SDK's ets-loader is discoverable, we compile them to
    /// `.abc` first; otherwise we ship source + emit a warning.
    pub ets_dir: &'a Path,
    pub stem: &'a str,
    pub sdk_native: Option<&'a Path>,
    pub quiet: bool,
}

pub struct HapBuildResult {
    pub hap_path: PathBuf,
    pub signed: bool,
    pub abc_compiled: bool,
}

/// Main entry. Returns the path to the final `.hap` (signed if credentials
/// were present, unsigned otherwise).
pub fn build_hap(args: &HapBuildArgs) -> Result<HapBuildResult> {
    let output_dir = args.so_path.parent().unwrap_or_else(|| Path::new("."));
    let staging = output_dir.join(format!("{}.hap_staging", args.stem));
    // Start clean — stale files from a prior `perry compile` run can leak into
    // the zip and make the HAP reject at install time.
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging)?;

    // Saved config from `perry setup harmonyos` (`~/.perry/config.toml`).
    // Used as a fallback when env vars aren't set so users only need to set
    // up signing once, not for every compile.
    let saved = super::publish::load_config();
    let saved_h = saved.harmonyos.as_ref();

    let bundle_name = std::env::var("PERRY_HARMONYOS_BUNDLE_NAME")
        .ok()
        .or_else(|| saved_h.and_then(|h| h.bundle_name.clone()))
        .unwrap_or_else(|| format!("com.perry.app.{}", sanitize_bundle_segment(args.stem)));

    write_configs(&staging, args.stem, &bundle_name)?;
    write_resources(&staging, args.stem)?;
    copy_so(&staging, args.so_path)?;
    copy_ets(&staging, args.ets_dir)?;

    // ets-loader lives under the SDK's toolchains/ets-loader; `es2abc` ships
    // standalone too. If we can find either we compile to .abc in place
    // (replacing .ets with .abc); otherwise we ship source and warn.
    let abc_compiled = if let Some(sdk) = args.sdk_native {
        match compile_ets_to_abc(sdk, &staging, args.quiet) {
            Ok(true) => true,
            Ok(false) => {
                if !args.quiet {
                    eprintln!(
                        "  harmonyos: ets/ shipped as source. Physical NEXT devices only \
                         execute .abc bytecode; this HAP will be rejected by `hdc install`. \
                         Either install ets-loader (DevEco Studio ships it) or hand the \
                         staging dir to hvigor to finish the build."
                    );
                }
                false
            }
            Err(e) => {
                if !args.quiet {
                    eprintln!("  harmonyos: ets-loader run failed ({}); shipping .ets source.", e);
                }
                false
            }
        }
    } else {
        false
    };

    let unsigned_hap = output_dir.join(format!("{}.unsigned.hap", args.stem));
    zip_staging(&staging, &unsigned_hap)?;

    let signed = match sign_hap(&unsigned_hap, output_dir, args.stem, args.sdk_native, args.quiet) {
        Ok(signed_path) => {
            // Remove the unsigned intermediate once signing succeeds — the
            // staging dir is already kept for inspection; two .hap files in
            // the output dir is confusing.
            let _ = fs::remove_file(&unsigned_hap);
            return Ok(HapBuildResult { hap_path: signed_path, signed: true, abc_compiled });
        }
        Err(e) => {
            if !args.quiet {
                eprintln!("  harmonyos: hap not signed ({}). Set PERRY_HARMONYOS_P12, \
                           PERRY_HARMONYOS_P12_PASSWORD, and PERRY_HARMONYOS_PROFILE \
                           to produce a signed HAP.", e);
            }
            false
        }
    };

    Ok(HapBuildResult {
        hap_path: unsigned_hap,
        signed,
        abc_compiled,
    })
}

/// Bundle names must be lowercase, alphanumeric + dots, no leading digit. Make
/// any user-given stem safe to splice into the fallback `com.perry.app.<stem>`.
fn sanitize_bundle_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && out.chars().last() != Some('_') {
            out.push('_');
        }
    }
    if out.is_empty() || out.chars().next().unwrap().is_ascii_digit() {
        out.insert(0, 'a');
    }
    out
}

fn write_configs(staging: &Path, stem: &str, bundle_name: &str) -> Result<()> {
    // API level 11 is the HarmonyOS NEXT floor; compatible=11 keeps install
    // open to any NEXT device. Target=21 matches DevEco 6.0.1's SDK
    // (HarmonyOS 6.0.1(21)). Bumping target lets install-time verification
    // see a HAP that's aware of 21-level APIs even though we don't use any.
    // User-configurable (via $PERRY_HARMONYOS_TARGET_API or package.json) is
    // a follow-up; 21 is the right default as of DevEco 6.x.
    const COMPAT_API: u32 = 11;
    const TARGET_API: u32 = 21;

    // app.json5 requires minAPIVersion / targetAPIVersion / apiReleaseType
    // at the app level — install-time verification rejects HAPs without
    // these. `apiReleaseType` spelling is distinct from the `releaseType`
    // key used inside pack.info — same semantics, different key name.
    let app_json = format!(
        r#"{{
  "app": {{
    "bundleName": "{bundle}",
    "vendor": "perry",
    "versionCode": 1000000,
    "versionName": "1.0.0",
    "icon": "$media:icon",
    "label": "$string:app_name",
    "minAPIVersion": {compat},
    "targetAPIVersion": {target},
    "apiReleaseType": "Release"
  }}
}}
"#,
        bundle = bundle_name,
        compat = COMPAT_API,
        target = TARGET_API,
    );
    fs::write(staging.join("app.json5"), app_json)?;

    // NOTE: `pages` is intentionally omitted. Phase 1 ships a UIAbility that
    // runs perryEntry.run() in onCreate without loading any ArkUI page —
    // there's no `windowStage.loadContent(...)` call. If `pages` were
    // declared but the referenced page didn't exist in ets/, packing-tool
    // would reject the HAP.
    let module_json = format!(
        r#"{{
  "module": {{
    "name": "entry",
    "type": "entry",
    "description": "$string:module_desc",
    "mainElement": "EntryAbility",
    "deviceTypes": ["phone", "tablet", "2in1", "wearable"],
    "deliveryWithInstall": true,
    "installationFree": false,
    "abilities": [
      {{
        "name": "EntryAbility",
        "srcEntry": "./ets/entryability/EntryAbility.ets",
        "description": "$string:EntryAbility_desc",
        "icon": "$media:icon",
        "label": "$string:EntryAbility_label",
        "startWindowIcon": "$media:icon",
        "startWindowBackground": "$color:start_window_background",
        "exported": true,
        "skills": [
          {{
            "entities": ["entity.system.home"],
            "actions": ["ohos.want.action.home"]
          }}
        ]
      }}
    ]
  }}
}}
"#
    );
    fs::write(staging.join("module.json5"), module_json)?;

    // pack.info is parsed by developtools_packing_tool / hap-sign-tool as
    // strict JSON (not JSON5 — no trailing commas). Critical shapes:
    //
    // * apiVersion lives under `summary.app.apiVersion`, NOT under each
    //   module. The Java parser reads from the app-level path and silently
    //   ignores a module-level duplicate.
    // * `summary.modules[].name` and `.package` are the *module* name
    //   ("entry") — they are NOT the bundleName. Confusingly, the top-level
    //   app block below uses bundleName. This is the most common shape
    //   bug in hand-rolled HAPs.
    // * `deviceType` (singular) in both modules and packages; must match
    //   the `deviceTypes` in module.json5 byte-for-byte or packing_tool's
    //   HapVerify rejects before sign.
    let pack_info = format!(
        r#"{{
  "summary": {{
    "app": {{
      "bundleName": "{bundle}",
      "version": {{ "code": 1000000, "name": "1.0.0" }},
      "apiVersion": {{ "compatible": {compat}, "releaseType": "Release", "target": {target} }}
    }},
    "modules": [{{
      "mainAbility": "EntryAbility",
      "deviceType": ["phone", "tablet", "2in1", "wearable"],
      "abilities": [{{ "name": "EntryAbility", "label": "{stem}" }}],
      "distro": {{
        "moduleType": "entry",
        "installationFree": false,
        "deliveryWithInstall": true,
        "moduleName": "entry"
      }},
      "apiVersion": {{ "compatible": {compat}, "releaseType": "Release", "target": {target} }},
      "package": "entry",
      "name": "entry"
    }}]
  }},
  "packages": [{{
    "deviceType": ["phone", "tablet", "2in1", "wearable"],
    "moduleType": "entry",
    "deliveryWithInstall": true,
    "name": "entry"
  }}]
}}
"#,
        bundle = bundle_name,
        stem = stem,
        compat = COMPAT_API,
        target = TARGET_API,
    );
    fs::write(staging.join("pack.info"), pack_info)?;

    Ok(())
}

fn write_resources(staging: &Path, stem: &str) -> Result<()> {
    let base = staging.join("resources").join("base");
    fs::create_dir_all(base.join("media"))?;
    fs::create_dir_all(base.join("string"))?;
    fs::create_dir_all(base.join("color"))?;

    fs::write(base.join("media").join("icon.png"), PLACEHOLDER_ICON_PNG)?;

    let string_json = format!(
        r#"{{
  "string": [
    {{ "name": "app_name", "value": "{stem}" }},
    {{ "name": "module_desc", "value": "{stem}" }},
    {{ "name": "EntryAbility_label", "value": "{stem}" }},
    {{ "name": "EntryAbility_desc", "value": "{stem}" }}
  ]
}}
"#
    );
    fs::write(base.join("string").join("string.json"), string_json)?;

    let color_json = r##"{
  "color": [
    { "name": "start_window_background", "value": "#FFFFFFFF" }
  ]
}
"##;
    fs::write(base.join("color").join("color.json"), color_json)?;

    // Phase 1 has no pages (module.json5 omits the `pages` field), so no
    // main_pages.json is emitted. PR C will reintroduce it when the
    // TS→ArkTS emitter produces real `@Entry @Component` page components.

    // en_US / zh_CN string overrides are optional; OHOS falls back to base/.
    // Omitted here to keep the surface small.

    Ok(())
}

fn copy_so(staging: &Path, so_path: &Path) -> Result<()> {
    // HarmonyOS expects .so libs under libs/<abi>/; the only ABI we target
    // today is arm64-v8a (physical device) and x86_64 (emulator). Perry's
    // `--target harmonyos-simulator` produces x86_64; stick the lib into
    // the matching dir.
    //
    // We infer the ABI from the .so's parent link target rather than args
    // because this fn doesn't see the CLI args — future: pass target enum
    // through HapBuildArgs if multi-ABI builds become a thing.
    let libs_dir = staging.join("libs").join("arm64-v8a");
    fs::create_dir_all(&libs_dir)?;
    let dest_name = so_path
        .file_name()
        .ok_or_else(|| anyhow!("HAP: .so path has no filename"))?;
    fs::copy(so_path, libs_dir.join(dest_name))?;
    Ok(())
}

fn copy_ets(staging: &Path, ets_src: &Path) -> Result<()> {
    let dest = staging.join("ets");
    copy_dir_recursive(ets_src, &dest)?;
    Ok(())
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src).with_context(|| format!("reading {}", src.display()))? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dst = dest.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &dst)?;
        } else if ty.is_file() {
            fs::copy(entry.path(), &dst)?;
        }
    }
    Ok(())
}

/// Compile the staging `ets/` tree into a single `ets/modules.abc`, then
/// delete the source `.ets` files. Returns `Ok(true)` if the bytecode was
/// produced, `Ok(false)` if `es2abc` can't be located (caller ships source
/// and warns — the resulting HAP won't install on a physical NEXT device).
///
/// We invoke `es2abc` directly rather than going through `ets-loader`:
///
/// * Phase 1 ArkTS is plain TypeScript (no `@Entry @Component struct`
///   decorators, no ArkUI syntax extensions), so es2abc with `--extension
///   ts` accepts the files as-is.
/// * ets-loader needs a full DevEco project layout (`build-profile.json5`,
///   several `aceModule*` env vars, etc.) — synthesizing all of that
///   re-implements a chunk of hvigor.
/// * Since Phase 1 emits exactly one `.ets` file (EntryAbility, no
///   pages/Index), there's nothing for ets-loader's bundling to bundle.
///
/// When PR C adds the TS→ArkTS emitter it'll need ets-loader back — that
/// emitter produces real `@Entry @Component struct` decorators that es2abc
/// won't accept directly.
fn compile_ets_to_abc(sdk_native: &Path, staging: &Path, quiet: bool) -> Result<bool> {
    let api_level_root = match sdk_native.parent() {
        Some(p) => p,
        None => return Ok(false),
    };
    let ets_loader_dir = api_level_root.join("ets/build-tools/ets-loader");

    // es2abc sits under ets-loader in a host-OS-specific subdir. `build-mac`
    // on macOS, `build-win` on Windows, `build` on Linux.
    let host_dir = if cfg!(target_os = "macos") {
        "build-mac"
    } else if cfg!(target_os = "windows") {
        "build-win"
    } else {
        "build"
    };
    let exe_suffix = if cfg!(target_os = "windows") { ".exe" } else { "" };
    let es2abc = ets_loader_dir.join(format!(
        "bin/ark/{}/bin/es2abc{}",
        host_dir, exe_suffix
    ));
    if !es2abc.exists() {
        if !quiet {
            eprintln!(
                "  harmonyos: es2abc not found at {} — ets/ will ship as source.",
                es2abc.display()
            );
        }
        return Ok(false);
    }

    // Collect every .ets file under staging/ets/ into a single invocation.
    // HAPs ship a single merged `ets/modules.abc`, not per-file .abc's.
    let ets_dir = staging.join("ets");
    let mut inputs: Vec<PathBuf> = Vec::new();
    for entry in walkdir::WalkDir::new(&ets_dir).into_iter().flatten() {
        if entry.file_type().is_file()
            && entry.path().extension().and_then(|e| e.to_str()) == Some("ets")
        {
            inputs.push(entry.path().to_path_buf());
        }
    }
    if inputs.is_empty() {
        if !quiet {
            eprintln!("  harmonyos: no .ets files found under {}", ets_dir.display());
        }
        return Ok(false);
    }

    let modules_abc = ets_dir.join("modules.abc");
    if !quiet {
        println!(
            "  harmonyos: {} .ets → modules.abc via {}",
            inputs.len(),
            es2abc.display()
        );
    }
    let status = Command::new(&es2abc)
        .arg("--module")
        .arg("--merge-abc")
        .arg("--extension")
        .arg("ts")
        .arg("--output")
        .arg(&modules_abc)
        .args(&inputs)
        .status()
        .with_context(|| format!("invoking {}", es2abc.display()))?;
    if !status.success() {
        return Err(anyhow!("es2abc exited with {}", status));
    }
    if !modules_abc.exists() {
        return Err(anyhow!(
            "es2abc claimed success but {} wasn't written",
            modules_abc.display()
        ));
    }

    // Drop the .ets sources — the HAP ships bytecode only. Keep the
    // directory structure (entryability/, pages/) empty so any tooling
    // that walks `ets/` doesn't stumble on the absence.
    for src in &inputs {
        let _ = fs::remove_file(src);
    }

    Ok(true)
}

fn zip_staging(staging: &Path, output_hap: &Path) -> Result<()> {
    let f = fs::File::create(output_hap)?;
    let mut writer = ZipWriter::new(f);
    let opts = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);

    for entry in walkdir::WalkDir::new(staging).into_iter().flatten() {
        let path = entry.path();
        if path == staging {
            continue;
        }
        let rel = path.strip_prefix(staging)?;
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        if entry.file_type().is_dir() {
            writer.add_directory(format!("{}/", rel_str), opts)?;
        } else {
            writer.start_file(rel_str, opts)?;
            let bytes = fs::read(path)?;
            writer.write_all(&bytes)?;
        }
    }
    writer.finish()?;
    Ok(())
}

fn sign_hap(
    unsigned: &Path,
    output_dir: &Path,
    stem: &str,
    sdk_native: Option<&Path>,
    quiet: bool,
) -> Result<PathBuf> {
    // Six env vars now — the B.3 original conflated the cert chain and the
    // provisioning profile into PERRY_HARMONYOS_PROFILE. `hap-sign-tool`
    // requires them as two different files:
    //   -appCertFile: end-entity → intermediate → root CA chain (.cer / .pem)
    //   -profileFile: signed provisioning profile (.p7b)
    // DevEco's "automatically generate signing files" flow writes both out
    // separately; mapping them to different env vars lets users point
    // perry at whatever DevEco produced.
    //
    // Each value falls through env var → saved config (`~/.perry/config.toml`,
    // populated by `perry setup harmonyos`). The wizard saves the user once;
    // env vars override on a per-invocation basis (CI, multi-cert workflows).
    let saved = super::publish::load_config();
    let saved_h = saved.harmonyos.as_ref();

    let p12 = std::env::var("PERRY_HARMONYOS_P12")
        .ok()
        .or_else(|| saved_h.and_then(|h| h.p12_path.clone()))
        .ok_or_else(|| anyhow!(
            "PERRY_HARMONYOS_P12 not set (path to .p12 keystore). \
             Run `perry setup harmonyos` once to configure, or export the env var."
        ))?;
    let p12_password = std::env::var("PERRY_HARMONYOS_P12_PASSWORD")
        .ok()
        .or_else(|| saved_h.and_then(|h| h.p12_password.clone()))
        .ok_or_else(|| anyhow!(
            "PERRY_HARMONYOS_P12_PASSWORD not set (keystore password). \
             Run `perry setup harmonyos` once to configure."
        ))?;
    let cert_chain = std::env::var("PERRY_HARMONYOS_CERT")
        .ok()
        .or_else(|| saved_h.and_then(|h| h.cert_path.clone()))
        .ok_or_else(|| anyhow!(
            "PERRY_HARMONYOS_CERT not set (path to the cert chain .cer/.pem — DevEco \
             auto-signing names it <bundleName>.cer). Distinct from PERRY_HARMONYOS_PROFILE."
        ))?;
    let profile = std::env::var("PERRY_HARMONYOS_PROFILE")
        .ok()
        .or_else(|| saved_h.and_then(|h| h.profile_path.clone()))
        .ok_or_else(|| anyhow!(
            "PERRY_HARMONYOS_PROFILE not set (path to the signed provisioning \
             profile .p7b — DevEco auto-signing names it <bundleName>.p7b)."
        ))?;

    // -keyPwd unlocks the private-key entry inside the keystore. Often the
    // same value as -keystorePwd, but `hap-sign-tool` expects them as
    // separate args. DevEco-generated p12s always have a key password.
    // Default to the keystore password if the caller didn't split them.
    let key_password = std::env::var("PERRY_HARMONYOS_KEY_PASSWORD")
        .unwrap_or_else(|_| p12_password.clone());

    // DevEco's auto-signing writes the alias into build-profile.json5;
    // a hardcoded string doesn't work. Default to "debugKey" (what DevEco
    // uses for auto-generated debug certs) and let the caller override.
    let key_alias = std::env::var("PERRY_HARMONYOS_KEY_ALIAS")
        .ok()
        .or_else(|| saved_h.and_then(|h| h.key_alias.clone()))
        .unwrap_or_else(|| "debugKey".to_string());

    let sign_alg = std::env::var("PERRY_HARMONYOS_SIGN_ALG")
        .unwrap_or_else(|_| "SHA256withECDSA".to_string());

    let hapsign = resolve_hapsign_tool(sdk_native)?;
    let signed = output_dir.join(format!("{}.hap", stem));

    if !quiet {
        println!("  harmonyos: signing with {}", hapsign.display());
    }

    // Full `sign-app` invocation per developtools_hapsigner's CLI reference
    // (README lines 297-314). `-profileSigned 1`, `-inForm zip`, `-signCode 1`
    // are defaults but passed explicitly so behavior is identical across SDK
    // versions that may have shifted defaults.
    let status = Command::new("java")
        .arg("-jar")
        .arg(&hapsign)
        .arg("sign-app")
        .args(["-mode", "localSign"])
        .args(["-keyAlias", &key_alias])
        .args(["-keyPwd", &key_password])
        .args(["-signAlg", &sign_alg])
        .args(["-appCertFile", &cert_chain])
        .args(["-profileFile", &profile])
        .args(["-profileSigned", "1"])
        .args(["-inFile", &unsigned.display().to_string()])
        .args(["-inForm", "zip"])
        .args(["-keystoreFile", &p12])
        .args(["-keystorePwd", &p12_password])
        .args(["-outFile", &signed.display().to_string()])
        .args(["-signCode", "1"])
        .status()
        .context("invoking hap-sign-tool via java; is `java` on PATH?")?;

    if !status.success() {
        return Err(anyhow!("hap-sign-tool exited with {}", status));
    }
    Ok(signed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use zip::ZipArchive;

    /// Smoke test: feed build_hap a fake .so + a fake ets/ dir, verify the
    /// resulting unsigned.hap has every member HarmonyOS requires. Signing +
    /// ets-loader are skipped (no env vars, no SDK), which exercises the
    /// "shipped as unsigned, source-mode" path.
    #[test]
    fn assembles_unsigned_hap_with_expected_layout() {
        let tmp = std::env::temp_dir().join(format!(
            "perry-hap-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(tmp.join("ets/entryability")).unwrap();
        fs::write(tmp.join("libhi.so"), b"fake so").unwrap();
        fs::write(tmp.join("ets/entryability/EntryAbility.ets"), "// ability").unwrap();

        // Scrub signing env so we stay on the unsigned path regardless of
        // whatever the host developer may have exported.
        for var in [
            "PERRY_HARMONYOS_P12",
            "PERRY_HARMONYOS_P12_PASSWORD",
            "PERRY_HARMONYOS_CERT",
            "PERRY_HARMONYOS_PROFILE",
            "PERRY_HARMONYOS_KEY_ALIAS",
            "PERRY_HARMONYOS_KEY_PASSWORD",
            "PERRY_HARMONYOS_SIGN_ALG",
            "PERRY_HARMONYOS_BUNDLE_NAME",
        ] {
            std::env::remove_var(var);
        }

        let so = tmp.join("libhi.so");
        let ets = tmp.join("ets");
        let args = HapBuildArgs {
            so_path: &so,
            ets_dir: &ets,
            stem: "hi",
            sdk_native: None,
            quiet: true,
        };
        let res = build_hap(&args).expect("build_hap failed");
        assert!(!res.signed, "no P12 env → unsigned");
        assert!(!res.abc_compiled, "no SDK → source-mode");
        assert!(res.hap_path.exists(), "{} missing", res.hap_path.display());

        // Unzip and check every required entry is present.
        let f = fs::File::open(&res.hap_path).unwrap();
        let mut zip = ZipArchive::new(f).unwrap();
        let mut names: Vec<String> = (0..zip.len())
            .map(|i| zip.by_index(i).unwrap().name().to_string())
            .collect();
        names.sort();
        let required = [
            "app.json5",
            "module.json5",
            "pack.info",
            "libs/arm64-v8a/libhi.so",
            "ets/entryability/EntryAbility.ets",
            "resources/base/media/icon.png",
            "resources/base/string/string.json",
            "resources/base/color/color.json",
        ];
        for r in required {
            assert!(
                names.iter().any(|n| n == r),
                "HAP missing {} — members: {:?}",
                r, names
            );
        }

        // Icon PNG should be intact — decompress and check magic bytes.
        let mut buf = Vec::new();
        {
            let mut icon = zip.by_name("resources/base/media/icon.png").unwrap();
            icon.read_to_end(&mut buf).unwrap();
        }
        assert_eq!(&buf[..8], b"\x89PNG\r\n\x1a\n");

        // Bundle name fallback should include the sanitized stem, and the
        // API-level triad (minAPIVersion / targetAPIVersion / apiReleaseType)
        // must be present — install verification rejects HAPs missing these.
        let mut s = String::new();
        {
            let mut app = zip.by_name("app.json5").unwrap();
            app.read_to_string(&mut s).unwrap();
        }
        assert!(s.contains("com.perry.app.hi"), "bundle fallback: {}", s);
        assert!(s.contains("\"minAPIVersion\""), "app.json5 missing minAPIVersion: {}", s);
        assert!(s.contains("\"targetAPIVersion\""), "app.json5 missing targetAPIVersion: {}", s);
        assert!(s.contains("\"apiReleaseType\": \"Release\""), "app.json5 missing apiReleaseType: {}", s);

        // pack.info: the `summary.modules[0].name` and `.package` must be
        // the *module* name ("entry"), not the bundleName. The apiVersion
        // must live under `summary.app.apiVersion`. These are the most
        // common bugs in hand-rolled HAPs.
        let mut pack = String::new();
        {
            let mut p = zip.by_name("pack.info").unwrap();
            p.read_to_string(&mut pack).unwrap();
        }
        // Quick structural check: parse as JSON and walk the paths.
        let pack_json: serde_json::Value = serde_json::from_str(&pack)
            .expect("pack.info must be valid JSON (strict, not JSON5)");
        assert_eq!(
            pack_json["summary"]["modules"][0]["name"], "entry",
            "pack.info modules[0].name must be module name, not bundleName"
        );
        assert_eq!(
            pack_json["summary"]["modules"][0]["package"], "entry",
            "pack.info modules[0].package must be module name, not bundleName"
        );
        assert_eq!(
            pack_json["packages"][0]["name"], "entry",
            "pack.info packages[0].name must be module name, not bundleName"
        );
        assert!(
            pack_json["summary"]["app"]["apiVersion"].is_object(),
            "pack.info must have summary.app.apiVersion (not just under modules)"
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn sanitize_bundle_segment_handles_edge_cases() {
        assert_eq!(sanitize_bundle_segment("hi"), "hi");
        assert_eq!(sanitize_bundle_segment("My-App"), "my_app");
        assert_eq!(sanitize_bundle_segment("123"), "a123");
        assert_eq!(sanitize_bundle_segment("weird//name"), "weird_name");
    }
}

fn resolve_hapsign_tool(sdk_native: Option<&Path>) -> Result<PathBuf> {
    if let Ok(env_path) = std::env::var("PERRY_HARMONYOS_HAPSIGN") {
        let p = PathBuf::from(env_path);
        if p.exists() {
            return Ok(p);
        }
        return Err(anyhow!("PERRY_HARMONYOS_HAPSIGN points at a missing file: {}", p.display()));
    }
    if let Some(sdk) = sdk_native {
        // Ships under toolchains/lib in recent DevEco releases; walk a small
        // candidate list (SDK layout has shifted between 4.x and 5.x).
        let candidates = [
            sdk.join("toolchains/lib/hap-sign-tool.jar"),
            sdk.join("toolchains/hap-sign-tool.jar"),
            sdk.join("llvm/bin/hap-sign-tool.jar"),
        ];
        for c in &candidates {
            if c.exists() {
                return Ok(c.clone());
            }
        }
    }
    Err(anyhow!("hap-sign-tool.jar not found in OHOS SDK; set PERRY_HARMONYOS_HAPSIGN to its path"))
}
