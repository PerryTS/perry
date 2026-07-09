//! Run command - compile and launch a TypeScript file in one step

use anyhow::{anyhow, bail, Context, Result};
use clap::Args;
use console::style;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::compile::{CompileArgs, CompileResult};
use crate::{OutputFormat, Platform};

mod android;
mod devices;
mod entry;
mod launch;
mod metadata;
mod remote;
mod resign;

pub use android::{build_and_run_android, build_and_run_wearos, install_and_launch_android};
pub use devices::{
    detect_android_devices, detect_booted_simulators, detect_booted_tv_simulators,
    detect_booted_visionos_simulators, detect_booted_watch_simulators, detect_ios_devices,
    is_wear_os_device, pick_device, pick_from_list, DeviceInfo,
};
pub use entry::{can_compile_locally, resolve_entry_file, resolve_target, rust_target_triple};
pub use launch::{launch, launch_ios_device, launch_ios_simulator, launch_native};
pub use metadata::{
    build_device_credentials, find_project_root, read_app_metadata, read_perry_toml_ios,
};
pub use remote::{copy_dir_recursive, remote_build_and_launch};
pub use resign::{
    create_dev_profile_via_api, read_ios_app_group_from_toml,
    read_ios_push_notifications_from_toml, resign_for_development, try_sign_existing_dev_profile,
};

#[derive(Args, Debug)]
pub struct RunArgs {
    /// Positional args: [platform] [input]. Platform is one of: macos, ios,
    /// visionos, watchos, tvos, android, wearos, linux, windows, web. If the first arg is not a
    /// known platform it is treated as the input file/directory.
    pub positional: Vec<String>,

    /// Specific iOS simulator UDID to target
    #[arg(long)]
    pub simulator: Option<String>,

    /// Specific iOS device UDID to target
    #[arg(long)]
    pub device: Option<String>,

    /// Enable WebAssembly host runtime (load .wasm at runtime via wasmi).
    #[arg(long)]
    pub enable_wasm_runtime: bool,

    /// Enable type checking via tsgo
    #[arg(long)]
    pub type_check: bool,

    /// C library for Linux targets: `glibc` (default) or `musl` (fully
    /// static). `--libc musl` upgrades a Linux target to its musl variant
    /// (e.g. `perry run linux --libc musl`). See #4826.
    #[arg(long)]
    pub libc: Option<String>,

    /// Force local compilation (error if toolchain missing)
    #[arg(long)]
    pub local: bool,

    /// Force remote compilation via Perry Hub build server
    #[arg(long)]
    pub remote: bool,

    /// Enable geisterhand in-process input fuzzer (debug/testing)
    #[arg(long)]
    pub enable_geisterhand: bool,

    /// Port for the geisterhand HTTP server (default: 7676).
    /// Implies --enable-geisterhand.
    #[arg(long)]
    pub geisterhand_port: Option<u16>,

    /// Arguments passed to the compiled program
    #[arg(last = true)]
    pub program_args: Vec<String>,
}

impl RunArgs {
    /// Parse the positional arguments into an optional platform and optional input path.
    /// If the first positional arg is a known platform name, it's used as the platform;
    /// otherwise it's treated as the input file/directory.
    pub fn parse_positional(&self) -> (Option<Platform>, Option<PathBuf>) {
        let mut iter = self.positional.iter();
        let first = match iter.next() {
            Some(v) => v,
            None => return (None, None),
        };

        // Try to parse as a platform
        if let Some(platform) = parse_platform(first) {
            let input = iter.next().map(PathBuf::from);
            (Some(platform), input)
        } else {
            // Not a platform — treat as input path
            (None, Some(PathBuf::from(first)))
        }
    }
}

pub fn parse_platform(s: &str) -> Option<Platform> {
    match s.to_lowercase().as_str() {
        "macos" => Some(Platform::Macos),
        "ios" => Some(Platform::Ios),
        "visionos" => Some(Platform::Visionos),
        "watchos" => Some(Platform::Watchos),
        "tvos" => Some(Platform::Tvos),
        "android" => Some(Platform::Android),
        "wearos" | "wear" | "wear-os" => Some(Platform::Wearos),
        "linux" => Some(Platform::Linux),
        "windows" => Some(Platform::Windows),
        "web" => Some(Platform::Web),
        _ => None,
    }
}

pub fn run(args: RunArgs, format: OutputFormat, use_color: bool, verbose: u8) -> Result<()> {
    // 0. Parse positional args into platform + input
    let (platform, input_path) = args.parse_positional();

    // 1. Resolve entry file
    let input = resolve_entry_file(input_path.as_deref())?;

    // 2. Resolve target and device
    let (target, device_udid) = resolve_target(platform, &args)?;

    // 3. Decide local vs remote compilation
    let needs_cross = matches!(
        target.as_deref(),
        Some("ios-simulator")
            | Some("ios")
            | Some("visionos-simulator")
            | Some("visionos")
            | Some("android")
            | Some("wearos")
            | Some("watchos-simulator")
            | Some("watchos")
            | Some("tvos-simulator")
            | Some("tvos")
    );
    let can_local = !needs_cross || can_compile_locally(target.as_deref());

    let use_remote = if args.remote {
        true
    } else if args.local {
        if !can_local {
            bail!(
                "Local compilation for {:?} requires cross-compiled runtime libraries.\n\
                 Build with: cargo build --release -p perry-runtime -p perry-stdlib --target {}\n\
                 Or use --remote to compile via Perry Hub.",
                target.as_deref().unwrap_or("native"),
                rust_target_triple(target.as_deref()).unwrap_or("unknown")
            );
        }
        false
    } else {
        // Auto-detect: use remote when local isn't possible
        needs_cross && !can_local
    };

    if use_remote {
        let target_str = target.as_deref().unwrap_or("native");
        let rt = tokio::runtime::Runtime::new()?;
        let result = rt.block_on(remote_build_and_launch(
            &input,
            target_str,
            device_udid.as_deref(),
            &args.program_args,
            args.enable_geisterhand || args.geisterhand_port.is_some(),
            args.geisterhand_port,
            format,
        ));
        return result;
    }

    // Read app metadata from perry.toml / package.json
    let project_dir = input
        .parent()
        .unwrap_or(Path::new("."))
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from("."));
    let project_root = find_project_root(&project_dir);
    let (app_name, bundle_id) = read_app_metadata(&project_root, &input);

    // Local compile path
    let compile_args = CompileArgs {
        input: input.clone(),
        output: Some(PathBuf::from(&app_name)),
        keep_intermediates: false,
        print_hir: false,
        no_link: false,
        no_codegen: false,
        enable_wasm_runtime: args.enable_wasm_runtime,
        target: target.clone(),
        libc: args.libc.clone(),
        march: None,
        app_bundle_id: Some(bundle_id),
        output_type: "executable".to_string(),
        bundle_extensions: None,
        embed: Vec::new(),
        type_check: args.type_check,
        minify: target.as_deref() == Some("web"),
        features: None,
        enable_geisterhand: args.enable_geisterhand || args.geisterhand_port.is_some(),
        geisterhand_port: args.geisterhand_port,
        minimal_stdlib: false,
        no_auto_optimize: false,
        debug_symbols: false,
        no_cache: false,
        // `perry run` has no `--cache-dir` flag; the resolver still honors
        // `PERRY_CACHE_DIR` / perry.toml `[perry] cacheDir` / package.json
        // `perry.cacheDir`.
        cache_dir: None,
        fast_math: false,
        fp_contract: None,
        verify_native_regions: false,
        disable_buffer_fast_path: false,
        explain_lowering: false,
        emit_attest: false,
        emit_sandbox: false,
        lockdown: false,
        strict_eval: false,
        strict_dynamic_import: false,
        strict_unimplemented: false,
        min_windows_version: "10".to_string(),
        windows_subsystem: "auto".to_string(),
        // Phase 2 v7: harmonyos signing flags. `perry run` is for local
        // iteration where unsigned HAPs are fine, so fall through to env
        // var / saved config without exposing CLI overrides here.
        p12_keystore: None,
        p12_password: None,
        harmonyos_cert: None,
        harmonyos_profile: None,
        harmonyos_key_alias: None,
        skip_swift_build: false,
        trace: None,
        focus: None,
    };

    let result = super::compile::run(compile_args, format, use_color, verbose)?;

    // Local iOS device builds need code signing before install
    if matches!(target.as_deref(), Some("ios") | Some("visionos")) {
        if let Some(udid) = device_udid.as_deref() {
            let config = super::publish::load_config();
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(resign_for_development(
                &result.output_path,
                &config,
                udid,
                format,
            ))?;
        }
    }

    launch(&result, device_udid.as_deref(), &args.program_args, format)
}
