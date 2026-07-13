//! Update command - check for and install Perry updates

use anyhow::Result;
use clap::Args;

use crate::update_checker;
use crate::OutputFormat;

#[derive(Args, Debug)]
pub struct UpdateArgs {
    /// Only check for updates, don't download
    #[arg(long)]
    pub check_only: bool,

    /// Ignore cache, always fetch from server
    #[arg(long)]
    pub force: bool,
}

pub fn run(
    args: UpdateArgs,
    format: OutputFormat,
    use_color: bool,
    verbose: u8,
    quiet: bool,
) -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");

    let status = if !args.force && !update_checker::is_cache_stale() {
        update_checker::check_cached_status()
    } else {
        match update_checker::spawn_background_check() {
            (handle, rx) => {
                let _ = handle.join();
                rx.recv()
                    .unwrap_or(update_checker::UpdateStatus::CheckFailed)
            }
        }
    };

    match status {
        update_checker::UpdateStatus::UpdateAvailable {
            current: cur,
            latest,
            release_url,
        } => {
            match format {
                OutputFormat::Json => {
                    let output = serde_json::json!({
                        "update_available": true,
                        "current_version": cur,
                        "latest_version": latest,
                        "release_url": release_url,
                    });
                    println!("{}", serde_json::to_string_pretty(&output)?);
                }
                // `--quiet` silences the informational Text chatter. It does NOT
                // silence `--format json` above: that is explicitly-requested
                // structured output, not chatter. Errors are never silenced.
                OutputFormat::Text if !quiet => {
                    if use_color {
                        println!(
                            "{} {} → {}",
                            console::style("Update available:").yellow().bold(),
                            cur,
                            console::style(&latest).green().bold(),
                        );
                    } else {
                        println!("Update available: {} -> {}", cur, latest);
                    }
                    println!("  Release: {}", release_url);
                }
                OutputFormat::Text => {}
            }

            if !args.check_only {
                if !quiet {
                    println!();
                }
                // Progress goes to stderr, so it never corrupts `--format json`
                // on stdout; `--quiet` silences it entirely.
                //
                // `quiet` beats `verbose`: `-q -v` must be silent, not verbose.
                // Otherwise the two flags fight and the louder one wins, which
                // is the opposite of what a user asking for quiet expects.
                update_checker::perform_self_update(update_checker::UpdateOutput {
                    verbose: verbose > 0 && !quiet,
                    quiet,
                    color: use_color,
                })?;
            }
        }
        update_checker::UpdateStatus::UpToDate => match format {
            OutputFormat::Json => {
                let output = serde_json::json!({
                    "update_available": false,
                    "current_version": current,
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
            }
            OutputFormat::Text if !quiet => {
                println!("Perry is up to date (v{})", current);
            }
            OutputFormat::Text => {}
        },
        update_checker::UpdateStatus::CheckFailed => match format {
            OutputFormat::Json => {
                let output = serde_json::json!({
                    "error": "Failed to check for updates",
                    "current_version": current,
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
            }
            OutputFormat::Text => {
                eprintln!("Failed to check for updates. Current version: v{}", current);
            }
        },
    }

    Ok(())
}
