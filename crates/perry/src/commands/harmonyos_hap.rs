//! HarmonyOS HAP (HarmonyOS Ability Package) bundler.
//!
//! Invoked by `compile` when `--target harmonyos[-simulator]` succeeds: takes
//! the already-linked `.so` + the ArkTS shim directory (both produced by
//! prior compile stages), assembles the OpenHarmony-required layout, optionally
//! compiles `.ets` → `.abc` via the SDK's ets-loader, zips the result as
//! `foo.hap`, and optionally runs `hap-sign` with user-supplied credentials.
//!
//! All signing credentials come from env vars — no config file in v1:
//!
//!   PERRY_HARMONYOS_P12           — path to signing key (.p12)
//!   PERRY_HARMONYOS_P12_PASSWORD  — password for the .p12
//!   PERRY_HARMONYOS_PROFILE       — path to `.p7b` provisioning profile
//!   PERRY_HARMONYOS_BUNDLE_NAME   — must match the cert's bundle name;
//!                                    falls back to `com.perry.app.<stem>`
//!                                    (which will only work with wildcard
//!                                    certs — unusable for real deploys).
//!   PERRY_HARMONYOS_HAPSIGN       — override path to the `hap-sign` binary;
//!                                    default: <sdk>/toolchains/lib/hap-sign-tool.jar
//!                                    (invoked via `java -jar ...`)
//!
//! If any signing env var is unset, the HAP is emitted unsigned (`<stem>.unsigned.hap`)
//! and a remediation message names the missing env vars. An unsigned HAP can't
//! be installed via `hdc install`; this mode is for inspection + iteration.

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

    let bundle_name = std::env::var("PERRY_HARMONYOS_BUNDLE_NAME")
        .unwrap_or_else(|_| format!("com.perry.app.{}", sanitize_bundle_segment(args.stem)));

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
                        "  harmonyos: ets-loader not found in SDK — shipping .ets source. \
                         The HAP will only install on a DevEco emulator with source-mode \
                         enabled, not on a physical NEXT device."
                    );
                }
                false
            }
            Err(e) => {
                if !args.quiet {
                    eprintln!("  harmonyos: ets-loader run failed ({}); shipping .ets source", e);
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
    let app_json = format!(
        r#"{{
  "app": {{
    "bundleName": "{bundle}",
    "vendor": "perry",
    "versionCode": 1000000,
    "versionName": "1.0.0",
    "icon": "$media:icon",
    "label": "$string:app_name"
  }}
}}
"#,
        bundle = bundle_name
    );
    fs::write(staging.join("app.json5"), app_json)?;

    let module_json = format!(
        r#"{{
  "module": {{
    "name": "entry",
    "type": "entry",
    "description": "$string:app_name",
    "mainElement": "EntryAbility",
    "deviceTypes": ["phone", "tablet", "2in1"],
    "deliveryWithInstall": true,
    "installationFree": false,
    "pages": "$profile:main_pages",
    "abilities": [
      {{
        "name": "EntryAbility",
        "srcEntry": "./ets/entryability/EntryAbility.ets",
        "description": "$string:app_name",
        "icon": "$media:icon",
        "label": "$string:EntryAbility_label",
        "startWindowIcon": "$media:icon",
        "startWindowBackground": "$color:start_window_background",
        "exported": true,
        "skills": [
          {{
            "entities": ["entity.system.home"],
            "actions": ["action.system.home"]
          }}
        ]
      }}
    ]
  }}
}}
"#
    );
    fs::write(staging.join("module.json5"), module_json)?;

    // pack.info is what hap-sign / hdc use to validate the bundle. Mirrors
    // hvigor's output for a minimal entry module.
    let pack_info = format!(
        r#"{{
  "summary": {{
    "app": {{
      "bundleName": "{bundle}",
      "version": {{ "code": 1000000, "name": "1.0.0" }}
    }},
    "modules": [{{
      "mainAbility": "EntryAbility",
      "deviceTypes": ["phone", "tablet"],
      "abilities": [{{ "name": "EntryAbility", "label": "{stem}" }}],
      "distro": {{
        "moduleType": "entry",
        "installationFree": false,
        "deliveryWithInstall": true,
        "moduleName": "entry"
      }},
      "apiVersion": {{ "compatible": 9, "releaseType": "Release", "target": 10 }},
      "package": "{bundle}",
      "name": "{bundle}"
    }}]
  }},
  "packages": [{{
    "deviceType": ["phone", "tablet"],
    "moduleType": "entry",
    "deliveryWithInstall": true,
    "name": "{bundle}"
  }}]
}}
"#,
        bundle = bundle_name,
        stem = stem,
    );
    fs::write(staging.join("pack.info"), pack_info)?;

    Ok(())
}

fn write_resources(staging: &Path, stem: &str) -> Result<()> {
    let base = staging.join("resources").join("base");
    fs::create_dir_all(base.join("media"))?;
    fs::create_dir_all(base.join("string"))?;
    fs::create_dir_all(base.join("color"))?;
    fs::create_dir_all(base.join("profile"))?;

    fs::write(base.join("media").join("icon.png"), PLACEHOLDER_ICON_PNG)?;

    let string_json = format!(
        r#"{{
  "string": [
    {{ "name": "app_name", "value": "{stem}" }},
    {{ "name": "EntryAbility_label", "value": "{stem}" }}
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

    // Tells the ArkTS runtime which page to load by default.
    let pages_json = r#"{
  "src": ["pages/Index"]
}
"#;
    fs::write(base.join("profile").join("main_pages.json"), pages_json)?;

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

/// Compile `.ets` → `.abc` via the OHOS SDK's ets-loader. Returns `Ok(true)`
/// if compilation ran, `Ok(false)` if the loader can't be located (caller
/// falls back to shipping source).
fn compile_ets_to_abc(sdk_native: &Path, staging: &Path, quiet: bool) -> Result<bool> {
    // Probe for the standalone Ark compiler first — it's a single binary
    // and doesn't need `node` on PATH. Ships under a few paths depending
    // on SDK version; walk a small set.
    let es2abc_candidates = [
        sdk_native.join("build-tools/ets-loader/bin/ark_ts2abc_bin/es2abc"),
        sdk_native.join("toolchains/lib/ark_tools/bin/es2abc"),
        sdk_native.join("toolchains/es2abc"),
        sdk_native.join("llvm/bin/es2abc"),
    ];
    let es2abc = es2abc_candidates.iter().find(|p| p.exists());

    if let Some(tool) = es2abc {
        if !quiet {
            println!("  harmonyos: compiling ets/ via {}", tool.display());
        }
        run_es2abc_over_dir(tool, &staging.join("ets"))?;
        return Ok(true);
    }

    // Node-based fallback: ets-loader shipped as a JS package.
    let ets_loader_main = sdk_native.join("build-tools/ets-loader/main.js");
    if ets_loader_main.exists() {
        if !quiet {
            println!(
                "  harmonyos: compiling ets/ via node {}",
                ets_loader_main.display()
            );
        }
        let status = Command::new("node")
            .arg(&ets_loader_main)
            .arg("--hap-mode=release")
            .arg(staging.join("ets"))
            .status()
            .context("running ets-loader; is `node` on PATH?")?;
        if !status.success() {
            return Err(anyhow!("ets-loader exited with {}", status));
        }
        return Ok(true);
    }

    Ok(false)
}

fn run_es2abc_over_dir(tool: &Path, ets_dir: &Path) -> Result<()> {
    for entry in walkdir::WalkDir::new(ets_dir).into_iter().flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("ets") {
            continue;
        }
        let out = path.with_extension("abc");
        let status = Command::new(tool)
            .arg("--module")
            .arg("--merge-abc")
            .arg(path)
            .arg("--output")
            .arg(&out)
            .status()
            .with_context(|| format!("invoking es2abc on {}", path.display()))?;
        if !status.success() {
            return Err(anyhow!("es2abc failed for {}", path.display()));
        }
        // Once bytecode is produced, drop the source — the HAP ships bytecode only.
        let _ = fs::remove_file(path);
    }
    Ok(())
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
    let p12 = std::env::var("PERRY_HARMONYOS_P12")
        .map_err(|_| anyhow!("PERRY_HARMONYOS_P12 not set"))?;
    let p12_password = std::env::var("PERRY_HARMONYOS_P12_PASSWORD")
        .map_err(|_| anyhow!("PERRY_HARMONYOS_P12_PASSWORD not set"))?;
    let profile = std::env::var("PERRY_HARMONYOS_PROFILE")
        .map_err(|_| anyhow!("PERRY_HARMONYOS_PROFILE not set"))?;

    let hapsign = resolve_hapsign_tool(sdk_native)?;
    let signed = output_dir.join(format!("{}.hap", stem));

    if !quiet {
        println!("  harmonyos: signing with {}", hapsign.display());
    }

    // Huawei's hap-sign-tool.jar CLI: `sign-app` subcommand, standard args.
    // See https://gitee.com/openharmony/developtools_hapsigner for the full
    // arg set; this is the minimum viable invocation.
    let status = Command::new("java")
        .arg("-jar")
        .arg(&hapsign)
        .arg("sign-app")
        .args(["-keyAlias", "perry-signing-key"])
        .args(["-signAlg", "SHA256withECDSA"])
        .args(["-mode", "localSign"])
        .args(["-appCertFile", &profile])
        .args(["-profileFile", &profile])
        .args(["-inFile", &unsigned.display().to_string()])
        .args(["-outFile", &signed.display().to_string()])
        .args(["-keystoreFile", &p12])
        .args(["-keystorePwd", &p12_password])
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
        fs::create_dir_all(tmp.join("ets/pages")).unwrap();
        fs::write(tmp.join("libhi.so"), b"fake so").unwrap();
        fs::write(tmp.join("ets/entryability/EntryAbility.ets"), "// ability").unwrap();
        fs::write(tmp.join("ets/pages/Index.ets"), "// index").unwrap();

        // Scrub signing env so we stay on the unsigned path regardless of
        // whatever the host developer may have exported.
        for var in [
            "PERRY_HARMONYOS_P12",
            "PERRY_HARMONYOS_P12_PASSWORD",
            "PERRY_HARMONYOS_PROFILE",
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
            "ets/pages/Index.ets",
            "resources/base/media/icon.png",
            "resources/base/string/string.json",
            "resources/base/color/color.json",
            "resources/base/profile/main_pages.json",
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

        // Bundle name fallback should include the sanitized stem.
        let mut s = String::new();
        {
            let mut app = zip.by_name("app.json5").unwrap();
            app.read_to_string(&mut s).unwrap();
        }
        assert!(s.contains("com.perry.app.hi"), "bundle fallback: {}", s);

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
