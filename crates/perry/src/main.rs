//! Perry - Native TypeScript Compiler
//!
//! CLI driver for compiling TypeScript to native executables.

mod commands;
mod telemetry;
mod update_checker;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};

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
    Linux,
    Windows,
    Web,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Compile TypeScript file(s) to native executable
    Compile(commands::compile::CompileArgs),

    /// Check TypeScript compatibility without compiling
    Check(commands::check::CheckArgs),

    /// Initialize a new perry project
    Init(commands::init::InitArgs),

    /// Check environment and dependencies
    Doctor(commands::doctor::DoctorArgs),

    /// Explain an error code
    Explain(commands::explain::ExplainArgs),

    /// Build, sign, package and publish your app
    Publish(commands::publish::PublishArgs),

    /// Set up credentials for App Store or Google Play distribution
    Setup(commands::setup::SetupArgs),

    /// Check for updates and self-update Perry
    Update(commands::update::UpdateArgs),

    /// Scan TypeScript source for security vulnerabilities
    Audit(commands::audit::AuditArgs),

    /// Submit compiled binary for runtime verification
    Verify(commands::verify::VerifyArgs),

    /// Compile and run a TypeScript file in one step
    Run(commands::run::RunArgs),

    /// Watch TypeScript source and auto-recompile on changes
    Dev(commands::dev::DevArgs),

    /// Internationalization tools (extract strings, manage locales)
    I18n(commands::i18n::I18nArgs),

    /// Log in to your Perry account (GitHub OAuth)
    Login(commands::login::LoginArgs),

    /// App Store management (release notes, metadata)
    Appstore(commands::appstore::AppStoreArgs),

    /// Generate TypeScript type stubs for Perry built-in modules
    Types(commands::types::TypesArgs),

    /// Manage the per-module object cache at `.perry-cache/`
    Cache(commands::cache::CacheArgs),
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
            "compile" | "check" | "init" | "doctor" | "explain" | "publish" | "update" | "setup" | "audit" | "verify" | "run" | "dev" | "appstore" | "types" | "cache" | "help"
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
    // Use a thread with a large stack (64 MB) to avoid stack overflow on large codebases
    let builder = std::thread::Builder::new()
        .name("perry-main".into())
        .stack_size(64 * 1024 * 1024);
    let handler = builder.spawn(|| main_inner()).unwrap();
    handler.join().unwrap()
}

fn main_inner() -> Result<()> {
    env_logger::init();

    // Handle legacy invocation (perry file.ts -o out)
    let args: Vec<String> = std::env::args().collect();
    let effective_args = if is_legacy_invocation(&args) {
        transform_legacy_args(args)
    } else {
        args
    };

    let cli = Cli::parse_from(effective_args);

    // Determine if colors should be used
    let use_color = !cli.no_color && !cli.quiet && atty::is(atty::Stream::Stdout);

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
                telemetry::send_event("compile", &[
                    ("platform", std::env::consts::OS),
                    ("target", &target),
                    ("version", env!("CARGO_PKG_VERSION")),
                    ("status", status),
                ]);
            }
            r.map(|_| ())
        }
        Commands::Run(args) => {
            commands::run::run(args, cli.format, use_color, cli.verbose)
        }
        Commands::Dev(args) => {
            commands::dev::run(args, cli.format, use_color, cli.verbose)
        }
        Commands::Check(args) => {
            commands::check::run(args, cli.format, use_color, cli.verbose)
        }
        Commands::Init(args) => {
            commands::init::run(args, cli.format, use_color)
        }
        Commands::Doctor(args) => {
            commands::doctor::run(args, cli.format, use_color)
        }
        Commands::Explain(args) => {
            commands::explain::run(args, cli.format, use_color)
        }
        Commands::Publish(args) => {
            commands::publish::run(args, cli.format, use_color, cli.verbose)
        }
        Commands::Setup(args) => {
            commands::setup::run(args)
        }
        Commands::Update(args) => {
            commands::update::run(args, cli.format, use_color, cli.verbose)
        }
        Commands::Audit(args) => {
            commands::audit::run(args, cli.format, use_color)
        }
        Commands::Verify(args) => {
            commands::verify::run(args, cli.format, use_color)
        }
        Commands::I18n(args) => {
            commands::i18n::run(args, cli.format)
        }
        Commands::Login(args) => {
            commands::login::run(args, cli.format, use_color)
        }
        Commands::Appstore(args) => {
            commands::appstore::run(args)
        }
        Commands::Types(args) => {
            commands::types::run(args, cli.format, use_color)
        }
        Commands::Cache(args) => {
            commands::cache::run(args, cli.format)
        }
    };

    // Send telemetry for non-compile commands (compile is handled above for target/status)
    if telemetry_active {
        if let Some(name) = command_name {
            if name != "compile" {
                telemetry::send_event(name, &[
                    ("platform", std::env::consts::OS),
                    ("version", env!("CARGO_PKG_VERSION")),
                ]);
            }
        }
    }

    // Print update notice if available (to stderr, non-blocking)
    if !cli.quiet && !is_update_cmd {
        let use_stderr_color = !cli.no_color && atty::is(atty::Stream::Stderr);
        let status = if let Some(rx) = bg_check {
            rx.recv_timeout(std::time::Duration::from_millis(100)).ok()
        } else if !update_checker::should_skip_check() {
            Some(update_checker::check_cached_status())
        } else {
            None
        };

        if let Some(update_checker::UpdateStatus::UpdateAvailable { current, latest, release_url }) = status {
            update_checker::print_update_notice(&current, &latest, &release_url, use_stderr_color);
        }
    }

    // Wait for any pending telemetry events to be delivered before exiting
    telemetry::flush();

    result
}
