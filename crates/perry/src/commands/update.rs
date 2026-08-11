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

    /// Save how Perry should handle updates from now on, then exit.
    ///
    /// This is the writable half of `[update] mode` in ~/.perry/config.toml —
    /// there to save people hand-editing TOML for the one setting they are
    /// most likely to want to change.
    #[arg(long, value_name = "off|notify|prompt|auto")]
    pub mode: Option<String>,
}

pub fn run(
    args: UpdateArgs,
    format: OutputFormat,
    use_color: bool,
    verbose: u8,
    quiet: bool,
) -> Result<()> {
    if let Some(raw) = args.mode.as_deref() {
        return set_mode(raw);
    }

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
                    if !release_url.is_empty() {
                        println!("  Release: {}", release_url);
                    }
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

/// Persist `[update] mode`, and nothing else.
///
/// Read-modify-write through the shared loader so the rest of the file — the
/// license key, the telemetry section, anything a newer Perry wrote — comes
/// back out the way it went in.
fn set_mode(raw: &str) -> Result<()> {
    let Some(mode) = crate::update_policy::UpdateMode::parse(raw) else {
        anyhow::bail!("unknown update mode `{raw}`. Valid values: off, notify, prompt, auto.");
    };
    if mode == crate::update_policy::UpdateMode::Unknown {
        anyhow::bail!("unknown update mode `{raw}`. Valid values: off, notify, prompt, auto.");
    }

    crate::commands::publish::update_config_file(|config| {
        config.update.get_or_insert_with(Default::default).mode = Some(mode);
    })?;

    let path = crate::commands::publish::config_path();
    println!("Update mode set to \"{raw}\" ({}).", path.display());
    if mode == crate::update_policy::UpdateMode::Auto {
        // Say the limits up front rather than letting someone discover them
        // the first time an update does not happen.
        println!(
            "Perry will install updates at the end of a successful run — except \
             on a package-manager-managed install, where it names that manager's \
             command instead."
        );
    }
    Ok(())
}
