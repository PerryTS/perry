//! Perry - Native TypeScript Compiler
//!
//! CLI driver for compiling TypeScript to native executables.

mod commands;
mod compat_reports;
mod telemetry;
mod update_checker;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use std::io::IsTerminal;

/// Native TypeScript Compiler
#[derive(Parser, Debug)]
#[command(name = "perry")]
#[command(author, version, about = "Compile TypeScript to native executables")]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Output format
    #[arg(long, global = true, default_value = "text")]
    format: OutputFormat,

    /// Increase verbosity (-v, -vv, -vvv)
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,

    /// Suppress non-error output
    #[arg(short, long, global = true)]
    quiet: bool,

    /// Disable colored output
    #[arg(long, global = true)]
    no_color: bool,

    /// Emit the structured manifest of supported stdlib APIs and exit.
    /// The same source-of-truth that the unimplemented-API check (#463)
    /// consults. Three formats:
    /// - `json` (default): structured machine-readable manifest;
    /// - `markdown`: Markdown reference page for docs (#465);
    /// - `dts`: TypeScript declaration file for editor squiggles.
    /// No subcommand is required — `perry --print-api-manifest` and
    /// `perry --print-api-manifest=markdown` both work on their own.
    #[arg(
        long,
        global = true,
        value_enum,
        num_args = 0..=1,
        default_missing_value = "json"
    )]
    print_api_manifest: Option<ApiManifestFormat>,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum ApiManifestFormat {
    Json,
    Markdown,
    Dts,
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
}

/// Target platform for run/publish commands.
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Platform {
    Macos,
    Ios,
    Visionos,
    Watchos,
    Tvos,
    Android,
    /// Wear OS — Android on a watch. Shares the perry-ui-android backend and
    /// `aarch64-linux-android` toolchain; only the APK packaging differs.
    #[value(alias = "wear", alias = "wear-os")]
    Wearos,
    Linux,
    Windows,
    Web,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Compile TypeScript file(s) to native executable
    #[command(alias = "build")]
    Compile(commands::compile::CompileArgs),

    /// Check TypeScript compatibility without compiling
    Check(commands::check::CheckArgs),

    /// Initialize a new perry project
    Init(commands::init::InitArgs),

    /// Install npm packages with a malware-scan gate.
    ///
    /// Wraps `bun install --ignore-scripts` (or `npm install --ignore-scripts`
    /// as fallback) so no package code executes during install. After
    /// extraction, scans `node_modules/` with bundled offline rules and
    /// only then runs lifecycle scripts — and only for packages on a
    /// curated trust allowlist. Works on any standard npm project; no
    /// Perry-specific config required.
    Install(commands::install::InstallArgs),

    /// Check environment and dependencies
    Doctor(commands::doctor::DoctorArgs),

    /// Explain an error code
    Explain(commands::explain::ExplainArgs),

    /// Build, sign, package and publish your app
    #[cfg(feature = "publish-cli")]
    Publish(commands::publish::PublishArgs),

    /// Set up credentials for App Store or Google Play distribution
    #[cfg(feature = "mobile-cli")]
    Setup(commands::setup::SetupArgs),

    /// Check for updates and self-update Perry
    Update(commands::update::UpdateArgs),

    /// Scan TypeScript source for security vulnerabilities
    #[cfg(feature = "audit-cli")]
    Audit(commands::audit::AuditArgs),

    /// Submit compiled binary for runtime verification
    #[cfg(feature = "audit-cli")]
    Verify(commands::verify::VerifyArgs),

    /// Compile and run a TypeScript file in one step
    Run(commands::run::RunArgs),

    /// Watch TypeScript source and auto-recompile on changes
    #[cfg(feature = "watch-cli")]
    Dev(commands::dev::DevArgs),

    /// Internationalization tools (extract strings, manage locales)
    I18n(commands::i18n::I18nArgs),

    /// Log in to your Perry account (GitHub OAuth)
    Login(commands::login::LoginArgs),

    /// App Store management (release notes, metadata)
    #[cfg(feature = "mobile-cli")]
    Appstore(commands::appstore::AppStoreArgs),

    /// Generate TypeScript type stubs for Perry built-in modules
    Types(commands::types::TypesArgs),

    /// Manage Perry's on-disk cache (default `node_modules/.cache/perry`)
    Cache(commands::cache::CacheArgs),

    /// Sign-side tooling for `@perry/updater` (closes #229).
    ///
    /// `perry updater keygen` — generate Ed25519 keypair.
    /// `perry updater sign`   — sign a binary for a v2 manifest entry.
    /// `perry updater verify` — sanity-check a v2 signature locally.
    /// `perry updater sign-cli-manifest` — sign an authenticated CLI update manifest.
    #[cfg(feature = "updater-cli")]
    Updater(commands::updater::UpdaterArgs),

    /// Native-bindings package tooling (#466 Phase 3).
    ///
    /// `perry native init <name>`  — scaffold a new wrapper package.
    /// `perry native validate`     — diff the manifest vs. the
    ///                                 staticlib's exported symbols.
    /// `perry native list`         — list bundled well-known bindings.
    #[cfg(feature = "native-cli")]
    Native(commands::native::NativeArgs),

    /// WidgetKit / Glance build glue (issue #676).
    ///
    /// `perry widget init <name>` — scaffold a SwiftUI WidgetKit source
    /// tree under `ios-widgets/<name>/` and append a `[[widget]]` entry
    /// to `perry.toml` so the next `perry compile --target ios` builds
    /// the widget and embeds the produced `.appex` under
    /// `<output>.app/Frameworks/`.
    #[cfg(feature = "mobile-cli")]
    Widget(commands::widget::WidgetArgs),

    /// Supply-chain lockfile for `perry.nativeLibrary` archives (#498).
    ///
    /// `perry lock`                 - verify-and-write the lockfile
    ///                                 (default mode); equivalent to
    ///                                 the gate `perry compile` runs.
    /// `perry lock --update <pkg>`  - refresh `<pkg>`'s hashes after
    ///                                 a deliberate upgrade.
    /// `perry lock --frozen`        - CI verification only; refuses to
    ///                                 extend the lockfile.
    Lock(commands::lock::LockArgs),
}

/// Check if the first non-flag argument looks like a TypeScript file
fn is_legacy_invocation(args: &[String]) -> bool {
    for arg in args.iter().skip(1) {
        // Skip flags
        if arg.starts_with('-') {
            continue;
        }
        // Check if it looks like a .ts file (and not a subcommand)
        if arg.ends_with(".ts") {
            return true;
        }
        // If it's a known subcommand, not legacy
        if matches!(
            arg.as_str(),
            "compile"
                | "check"
                | "build"
                | "init"
                | "doctor"
                | "explain"
                | "install"
                | "publish"
                | "update"
                | "setup"
                | "audit"
                | "verify"
                | "run"
                | "dev"
                | "appstore"
                | "types"
                | "cache"
                | "updater"
                | "native"
                | "widget"
                | "lock"
                | "help"
        ) {
            return false;
        }
        // First non-flag, non-subcommand arg
        break;
    }
    false
}

/// Transform legacy args (perry file.ts -o out) to subcommand form
fn transform_legacy_args(args: Vec<String>) -> Vec<String> {
    let mut new_args = vec![args[0].clone(), "compile".to_string()];
    new_args.extend(args.into_iter().skip(1));
    new_args
}

fn main() -> Result<()> {
    // Install a panic hook that prints a Perry-formatted `Error:` line before
    // the default backtrace dump. Without this, a panic deep in the compile
    // pipeline (or a stack overflow that DOES surface as a panic before the
    // runtime aborts) shows only the raw Rust panic message at the very end
    // of hundreds of "Warning:" lines — easy to miss, looks like a silent
    // exit to anyone scanning the tail of the output.
    //
    // Note: this hook does NOT fire for `fatal runtime error: stack overflow,
    // aborting` — that path is a libstd abort() that bypasses the Rust panic
    // infrastructure entirely. For that case, the join-error branch below
    // can't help either (abort kills the whole process; the parent thread
    // never returns). The best we can do for hard aborts is print a hint at
    // exit time, which we now do via a `Drop` guard in `main_inner`.
    install_panic_hook();

    // Use a thread with a large stack (128 MB) to avoid stack overflow on
    // large codebases. Bumped from 64 MB in v0.5.973 — ioredis-via-
    // compilePackages (~30 transitive CJS modules) overflowed 64 MB in the
    // collect/lower walk on the perry-main thread.
    //
    // PERRY_MAIN_STACK_MB overrides the size (in MB) without a rebuild —
    // useful both for diagnosing suspected-unbounded recursion (bump it and
    // see whether the overflow moves, #6593) and as an escape hatch for
    // legitimately deep inputs that outgrow the default (already happened
    // twice: v0.5.973, #6593). Invalid or zero values fall back to 128;
    // values above 16384 (16 GB) clamp so the MB→bytes conversion cannot
    // overflow usize and the spawn below cannot fail on an absurd request.
    const MAX_STACK_MB: usize = 16 * 1024;
    let stack_mb: usize = std::env::var("PERRY_MAIN_STACK_MB")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .filter(|mb| *mb > 0)
        .map(|mb: usize| {
            if mb > MAX_STACK_MB {
                eprintln!(
                    "perry: PERRY_MAIN_STACK_MB={mb} exceeds the {MAX_STACK_MB} MB ceiling; clamping"
                );
                MAX_STACK_MB
            } else {
                mb
            }
        })
        .unwrap_or(128);
    let stack_bytes = stack_mb
        .checked_mul(1024 * 1024)
        .expect("stack size in bytes fits usize (clamped above)");
    let builder = std::thread::Builder::new()
        .name("perry-main".into())
        .stack_size(stack_bytes);
    let handler = match builder.spawn(main_inner) {
        Ok(handler) => handler,
        Err(err) => {
            return Err(anyhow::anyhow!(
                "failed to spawn the perry-main compiler thread \
                 (stack size {stack_mb} MB; try a smaller PERRY_MAIN_STACK_MB): {err}"
            ));
        }
    };
    match handler.join() {
        Ok(result) => result,
        Err(panic_payload) => {
            // Worker thread panicked. Extract the panic message (if it's a
            // String/&str) and surface it as a Perry-formatted error. The
            // panic hook already printed the raw panic location; this gives
            // the user one final clearly-prefixed line so they don't have to
            // scroll back through the output to find it.
            Err(anyhow::anyhow!(
                "perry compiler panicked: {}",
                extract_panic_message(&panic_payload)
            ))
        }
    }
}

/// Extract a human-readable panic message from a payload returned by
/// `JoinHandle::join`. Rust panics carry either a `&'static str`, a `String`,
/// or an opaque payload (rare). Tested below.
fn extract_panic_message(payload: &Box<dyn std::any::Any + Send + 'static>) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "no message — see backtrace above".to_string()
    }
}

/// Install a panic hook that prepends an `Error:` line to the default panic
/// dump. Keeps the existing backtrace behaviour (RUST_BACKTRACE still works)
/// while making the failure mode obvious in the user's terminal.
fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        eprintln!();
        eprintln!("Error: perry crashed unexpectedly.");
        eprintln!("       Please report this at https://github.com/PerryTS/perry/issues");
        eprintln!("       with the command line you ran and the output below.");
        eprintln!();
        default_hook(info);
    }));
}

fn main_inner() -> Result<()> {
    env_logger::init();
    #[cfg(windows)]
    {
        let raw_args: Vec<String> = std::env::args().collect();
        if let Some(result) = update_checker::maybe_run_windows_update_helper(&raw_args) {
            return result;
        }
    }
    update_checker::recover_interrupted_self_update()?;

    // Handle legacy invocation (perry file.ts -o out)
    let args: Vec<String> = std::env::args().collect();
    let effective_args = if is_legacy_invocation(&args) {
        transform_legacy_args(args)
    } else {
        args
    };

    let cli = Cli::parse_from(effective_args);

    // Determine if colors should be used
    let use_color = !cli.no_color && !cli.quiet && std::io::stdout().is_terminal();

    // `--print-api-manifest[=<format>]` short-circuits before any
    // subcommand dispatch — emits the manifest in the requested format
    // and exits 0. Drives docs / .d.ts generation (#465) and lets
    // editor tooling discover the supported surface without reading
    // Rust source. Default format is JSON to preserve compatibility
    // with the bare-flag form added in v0.5.528.
    if let Some(format) = cli.print_api_manifest {
        let version = env!("CARGO_PKG_VERSION");
        match format {
            ApiManifestFormat::Json => {
                let entries: Vec<serde_json::Value> = perry_api_manifest::iter_entries()
                    .map(|entry| {
                        let mut value = serde_json::to_value(entry)?;
                        if let Some(object) = value.as_object_mut() {
                            object.insert(
                                "module_export".to_string(),
                                serde_json::json!(
                                    perry_api_manifest::entry_is_public_named_export(entry)
                                ),
                            );
                        }
                        Ok::<_, serde_json::Error>(value)
                    })
                    .collect::<Result<_, _>>()?;
                let payload = serde_json::json!({
                    "version": version,
                    "entries": entries,
                });
                println!("{}", serde_json::to_string_pretty(&payload)?);
            }
            ApiManifestFormat::Markdown => {
                print!("{}", perry_api_manifest::emit_markdown(version));
            }
            ApiManifestFormat::Dts => {
                print!("{}", perry_api_manifest::emit_dts(version));
            }
        }
        return Ok(());
    }

    // Handle no command case
    if cli.command.is_none() {
        let mut cmd = <Cli as clap::CommandFactory>::command();
        cmd.print_help()?;
        println!();
        return Ok(());
    }

    // Check telemetry consent (prompts once on first interactive run)
    let telemetry_active = if !cli.quiet {
        telemetry::init_and_check_consent()
    } else {
        false
    };

    // #849: install the compat-report sink so diagnostic emission sites
    // can enqueue reports. Honors `compatibility_reports = "off"` (skips
    // installation entirely) and `PERRY_NO_TELEMETRY=1`/`CI=true` (same
    // env overrides as the generic telemetry channel).
    compat_reports::install_sink();

    // Spawn background update check (non-blocking, cached for 24h)
    let is_update_cmd = matches!(cli.command, Some(Commands::Update(_)));
    let bg_check = if !cli.quiet && !is_update_cmd && !update_checker::should_skip_check() {
        if update_checker::is_cache_stale() {
            let (_handle, rx) = update_checker::spawn_background_check();
            Some(rx)
        } else {
            None // will check cache after command runs
        }
    } else {
        None
    };

    let command = cli.command.unwrap();
    let command_name = match &command {
        Commands::Compile(_) => Some("compile"),
        Commands::Init(_) => Some("init"),
        #[cfg(feature = "publish-cli")]
        Commands::Publish(_) => Some("publish"),
        Commands::Doctor(_) => Some("doctor"),
        Commands::Update(_) => Some("update"),
        Commands::Run(_) => Some("run"),
        _ => None, // check, explain, setup — no telemetry
    };

    let result = match command {
        Commands::Compile(args) => {
            let target = args.target.as_deref().unwrap_or("native").to_string();
            let r = commands::compile::run(args, cli.format, use_color, cli.verbose);
            if telemetry_active {
                let status = if r.is_ok() { "success" } else { "error" };
                telemetry::send_event(
                    "compile",
                    &[
                        ("platform", std::env::consts::OS),
                        ("target", &target),
                        ("version", env!("CARGO_PKG_VERSION")),
                        ("status", status),
                    ],
                );
            }
            r.map(|_| ())
        }
        Commands::Run(args) => commands::run::run(args, cli.format, use_color, cli.verbose),
        #[cfg(feature = "watch-cli")]
        Commands::Dev(args) => commands::dev::run(args, cli.format, use_color, cli.verbose),
        Commands::Check(args) => commands::check::run(args, cli.format, use_color, cli.verbose),
        Commands::Init(args) => commands::init::run(args, cli.format, use_color),
        Commands::Install(args) => commands::install::run(args, cli.format, use_color),
        Commands::Doctor(args) => commands::doctor::run(args, cli.format, use_color),
        Commands::Explain(args) => commands::explain::run(args, cli.format, use_color),
        #[cfg(feature = "publish-cli")]
        Commands::Publish(args) => commands::publish::run(args, cli.format, use_color, cli.verbose),
        #[cfg(feature = "mobile-cli")]
        Commands::Setup(args) => commands::setup::run(args),
        Commands::Update(args) => {
            commands::update::run(args, cli.format, use_color, cli.verbose, cli.quiet)
        }
        #[cfg(feature = "audit-cli")]
        Commands::Audit(args) => commands::audit::run(args, cli.format, use_color),
        #[cfg(feature = "audit-cli")]
        Commands::Verify(args) => commands::verify::run(args, cli.format, use_color),
        Commands::I18n(args) => commands::i18n::run(args, cli.format),
        Commands::Login(args) => commands::login::run(args, cli.format, use_color),
        #[cfg(feature = "mobile-cli")]
        Commands::Appstore(args) => commands::appstore::run(args),
        Commands::Types(args) => commands::types::run(args, cli.format, use_color),
        Commands::Cache(args) => commands::cache::run(args, cli.format),
        #[cfg(feature = "updater-cli")]
        Commands::Updater(args) => commands::updater::run(args),
        #[cfg(feature = "native-cli")]
        Commands::Native(args) => commands::native::run(args, cli.format, use_color),
        #[cfg(feature = "mobile-cli")]
        Commands::Widget(args) => commands::widget::run(args, cli.format, use_color),
        Commands::Lock(args) => commands::lock::run(args, cli.format, use_color),
    };

    // Send telemetry for non-compile commands (compile is handled above for target/status)
    if telemetry_active {
        if let Some(name) = command_name {
            if name != "compile" {
                telemetry::send_event(
                    name,
                    &[
                        ("platform", std::env::consts::OS),
                        ("version", env!("CARGO_PKG_VERSION")),
                    ],
                );
            }
        }
    }

    // Print update notice if available (to stderr, non-blocking)
    if !cli.quiet && !is_update_cmd {
        let use_stderr_color = !cli.no_color && std::io::stderr().is_terminal();
        let status = if let Some(rx) = bg_check {
            rx.recv_timeout(std::time::Duration::from_millis(100)).ok()
        } else if !update_checker::should_skip_check() {
            Some(update_checker::check_cached_status())
        } else {
            None
        };

        if let Some(update_checker::UpdateStatus::UpdateAvailable {
            current,
            latest,
            release_url,
        }) = status
        {
            update_checker::print_update_notice(&current, &latest, &release_url, use_stderr_color);
        }
    }

    // #849: drain queued compat reports (prompts the user once in `ask`
    // mode) before flushing generic telemetry. Runs even when the
    // command succeeded — reports fire on *warnings* too (e.g. NoOpStub).
    compat_reports::flush();

    // Wait for any pending telemetry events to be delivered before exiting
    telemetry::flush();

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: ioredis-via-compilePackages (and any other "worker panics
    /// deep in the pipeline" shape) used to surface as a stack overflow with
    /// no Perry-prefixed error message. The join-error branch in `main()`
    /// now wraps any worker panic in a clear `perry compiler panicked: …`
    /// anyhow error so the user sees something obvious at exit time, even
    /// when the panic message itself is buried in thousands of compile
    /// warnings.
    #[test]
    fn extract_panic_message_handles_string_panic() {
        let handle = std::thread::spawn(|| {
            panic!("synthetic panic from String");
        });
        let err = handle.join().expect_err("thread should panic");
        let msg = extract_panic_message(&err);
        assert_eq!(msg, "synthetic panic from String");
    }

    #[test]
    fn extract_panic_message_handles_static_str_panic() {
        let handle = std::thread::spawn(|| {
            // panic_any with a &'static str payload — the no-format-args path.
            std::panic::panic_any("a static str");
        });
        let err = handle.join().expect_err("thread should panic");
        let msg = extract_panic_message(&err);
        assert_eq!(msg, "a static str");
    }

    #[test]
    fn extract_panic_message_handles_opaque_payload() {
        // panic_any with a non-string payload — most panic-hook frameworks
        // box something weird in here. Confirm we don't crash and emit the
        // documented fallback so the user at least sees `Error: …` instead
        // of a silent abort.
        let handle = std::thread::spawn(|| {
            std::panic::panic_any(42_i32);
        });
        let err = handle.join().expect_err("thread should panic");
        let msg = extract_panic_message(&err);
        assert!(msg.contains("no message"), "got {:?}", msg);
    }
}
