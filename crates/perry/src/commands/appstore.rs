//! App Store management commands

use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use std::fs;

#[derive(Args, Debug)]
pub struct AppStoreArgs {
    #[command(subcommand)]
    pub command: AppStoreCommand,

    /// Project directory
    #[arg(long, default_value = ".")]
    pub project: String,
}

#[derive(Subcommand, Debug)]
pub enum AppStoreCommand {
    /// Set or update "What's New" release notes for App Store / Google Play
    UpdateNotes {
        /// The release notes text
        text: String,

        /// Locale to set (e.g. "en", "de"). If omitted, sets for all configured i18n locales.
        #[arg(long)]
        locale: Option<String>,
    },

    /// Show current release notes
    ShowNotes,

    /// Clear all release notes
    ClearNotes,
}

pub fn run(args: AppStoreArgs) -> Result<()> {
    let project_dir = std::path::PathBuf::from(&args.project);
    let toml_path = project_dir.join("perry.toml");

    if !toml_path.exists() {
        bail!("No perry.toml found in {}", project_dir.display());
    }

    let content = fs::read_to_string(&toml_path)?;

    match args.command {
        AppStoreCommand::UpdateNotes { text, locale } => {
            update_notes(&toml_path, &content, &text, locale.as_deref())
        }
        AppStoreCommand::ShowNotes => show_notes(&content),
        AppStoreCommand::ClearNotes => clear_notes(&toml_path, &content),
    }
}

fn update_notes(
    toml_path: &std::path::Path,
    content: &str,
    text: &str,
    locale: Option<&str>,
) -> Result<()> {
    // Parse existing TOML to find i18n locales
    let parsed: toml::Value = toml::from_str(content)?;

    let locales: Vec<String> = if let Some(loc) = locale {
        vec![loc.to_string()]
    } else {
        // Get locales from [i18n] section, fall back to just "en"
        parsed
            .get("i18n")
            .and_then(|i| i.get("locales"))
            .and_then(|l| l.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_else(|| vec!["en".to_string()])
    };

    // Build the [release_notes] TOML section
    let mut notes_section = String::from("\n[release_notes]\n");
    for loc in &locales {
        // Escape the text for TOML (use triple-quoted strings for multiline safety)
        if text.contains('\n') {
            notes_section.push_str(&format!("{} = \"\"\"\n{}\"\"\"\n", loc, text));
        } else {
            let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
            notes_section.push_str(&format!("{} = \"{}\"\n", loc, escaped));
        }
    }

    // Replace existing [release_notes] section or append
    let new_content = if content.contains("[release_notes]") {
        // Find the section and replace it (up to the next [section] or end of file)
        let start = content.find("[release_notes]").unwrap();
        // Find the next section header after [release_notes]
        let rest = &content[start + "[release_notes]".len()..];
        let end = rest
            .find("\n[")
            .map(|pos| start + "[release_notes]".len() + pos)
            .unwrap_or(content.len());
        format!(
            "{}{}{}",
            &content[..start].trim_end(),
            notes_section,
            &content[end..]
        )
    } else {
        format!("{}\n{}", content.trim_end(), notes_section)
    };

    fs::write(toml_path, new_content)?;

    println!("Updated release notes for {} locale(s):", locales.len());
    for loc in &locales {
        println!(
            "  {} — {}",
            loc,
            if text.len() > 60 {
                format!("{}...", &text[..57])
            } else {
                text.to_string()
            }
        );
    }
    Ok(())
}

fn show_notes(content: &str) -> Result<()> {
    let parsed: toml::Value = toml::from_str(content)?;

    if let Some(notes) = parsed.get("release_notes").and_then(|n| n.as_table()) {
        if notes.is_empty() {
            println!("No release notes set.");
            return Ok(());
        }
        println!("Current release notes:");
        for (locale, text) in notes {
            if let Some(s) = text.as_str() {
                println!("  [{}] {}", locale, s);
            }
        }
    } else {
        println!("No [release_notes] section in perry.toml.");
    }
    Ok(())
}

fn clear_notes(toml_path: &std::path::Path, content: &str) -> Result<()> {
    if !content.contains("[release_notes]") {
        println!("No release notes to clear.");
        return Ok(());
    }

    let start = content.find("[release_notes]").unwrap();
    let rest = &content[start + "[release_notes]".len()..];
    let end = rest
        .find("\n[")
        .map(|pos| start + "[release_notes]".len() + pos)
        .unwrap_or(content.len());
    let new_content = format!("{}{}", content[..start].trim_end(), &content[end..]);
    fs::write(toml_path, new_content.trim_end().to_string() + "\n")?;
    println!("Release notes cleared.");
    Ok(())
}
