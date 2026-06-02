//! i18n CLI commands — extract localizable strings and manage locale files.

use anyhow::{anyhow, Result};
use clap::{Args, Subcommand};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::OutputFormat;

#[derive(Args, Debug)]
pub struct I18nArgs {
    #[command(subcommand)]
    pub command: I18nCommand,
}

#[derive(Subcommand, Debug)]
pub enum I18nCommand {
    /// Extract localizable strings from source files and update locale JSON scaffolds
    Extract(ExtractArgs),
}

#[derive(Args, Debug)]
pub struct ExtractArgs {
    /// Entry TypeScript file (defaults to src/main.ts)
    #[arg(default_value = "src/main.ts")]
    pub input: PathBuf,
}

/// UI widget constructors whose first string argument is localizable.
const LOCALIZABLE_WIDGETS: &[&str] = &[
    "Button",
    "Text",
    "Label",
    "TextField",
    "TextArea",
    "Tab",
    "NavigationTitle",
    "SectionHeader",
    "SecureField",
    "Alert",
];

pub fn run(args: I18nArgs, format: OutputFormat) -> Result<()> {
    match args.command {
        I18nCommand::Extract(extract_args) => run_extract(extract_args, format),
    }
}

fn run_extract(args: ExtractArgs, format: OutputFormat) -> Result<()> {
    let input = args
        .input
        .canonicalize()
        .map_err(|_| anyhow!("Input file not found: {}", args.input.display()))?;

    // Find project root (where perry.toml lives)
    let mut project_root = input.parent().unwrap_or(Path::new(".")).to_path_buf();
    for _ in 0..5 {
        if project_root.join("perry.toml").exists() {
            break;
        }
        if let Some(parent) = project_root.parent() {
            project_root = parent.to_path_buf();
        } else {
            break;
        }
    }

    // Read i18n config from perry.toml
    let toml_path = project_root.join("perry.toml");
    if !toml_path.exists() {
        return Err(anyhow!("perry.toml not found. Run `perry init` first."));
    }

    let toml_content = fs::read_to_string(&toml_path)?;
    let doc = toml_content
        .parse::<toml::Table>()
        .map_err(|e| anyhow!("Failed to parse perry.toml: {}", e))?;

    let i18n = doc.get("i18n").and_then(|v| v.as_table())
        .ok_or_else(|| anyhow!("No [i18n] section in perry.toml. Add:\n\n[i18n]\nlocales = [\"en\", \"de\"]\ndefault_locale = \"en\""))?;

    let locales: Vec<String> = i18n
        .get("locales")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let default_locale = i18n
        .get("default_locale")
        .and_then(|v| v.as_str())
        .unwrap_or("en")
        .to_string();

    if locales.is_empty() {
        return Err(anyhow!("No locales configured in [i18n].locales"));
    }

    // Scan source files for localizable strings
    match format {
        OutputFormat::Text => println!("Scanning for localizable strings..."),
        OutputFormat::Json => {}
    }

    let mut keys: BTreeSet<String> = BTreeSet::new();
    scan_file_for_keys(&input, &mut keys)?;

    // Also scan imported files (simple: scan all .ts files in the source directory)
    if let Some(src_dir) = input.parent() {
        scan_directory_for_keys(src_dir, &mut keys)?;
    }

    match format {
        OutputFormat::Text => println!("  Found {} localizable string(s)", keys.len()),
        OutputFormat::Json => {}
    }

    // Update locale files
    let locales_dir = project_root.join("locales");
    let _ = fs::create_dir_all(&locales_dir);

    for locale in &locales {
        let locale_file = locales_dir.join(format!("{}.json", locale));
        let mut existing: BTreeMap<String, String> = if locale_file.exists() {
            let content = fs::read_to_string(&locale_file)?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            BTreeMap::new()
        };

        let mut new_count = 0;
        // #854: `removed_count` was init'd to 0 then unconditionally
        // overwritten — declared without an initial value.
        let removed_count: usize;

        // Add new keys
        for key in &keys {
            if !existing.contains_key(key) {
                if locale == &default_locale {
                    existing.insert(key.clone(), key.clone()); // Default locale: key = value
                } else {
                    existing.insert(key.clone(), String::new()); // Non-default: empty = needs translation
                }
                new_count += 1;
            }
        }

        // Count removed keys (keys in file but not in source)
        let stale: Vec<String> = existing
            .keys()
            .filter(|k| !keys.contains(*k))
            .cloned()
            .collect();
        removed_count = stale.len();

        // Write updated file (keep stale keys but report them)
        let json = serde_json::to_string_pretty(&existing)?;
        fs::write(&locale_file, format!("{}\n", json))?;

        match format {
            OutputFormat::Text => {
                println!(
                    "  Updated locales/{}.json ({} new, {} unused)",
                    locale, new_count, removed_count
                );
            }
            OutputFormat::Json => {}
        }
    }

    match format {
        OutputFormat::Text => println!("Done."),
        OutputFormat::Json => {
            let result = serde_json::json!({
                "keys": keys.len(),
                "locales": locales.len(),
            });
            println!("{}", serde_json::to_string(&result)?);
        }
    }

    Ok(())
}

/// Scan a single TypeScript file for localizable string patterns.
fn scan_file_for_keys(path: &Path, keys: &mut BTreeSet<String>) -> Result<()> {
    let content = fs::read_to_string(path)
        .map_err(|e| anyhow!("Failed to read {}: {}", path.display(), e))?;

    // Simple regex-free scanning for patterns like:
    // Button("string"), Text("string"), Label("string"), etc.
    // Also: i18n.t("string") or t("string")
    for widget in LOCALIZABLE_WIDGETS {
        let pattern = format!("{}(\"", widget);
        extract_string_args(&content, &pattern, keys);
        // Single-quote variant
        let pattern_sq = format!("{}('", widget);
        extract_string_args_sq(&content, &pattern_sq, keys);
    }
    // t("string") pattern
    extract_string_args(&content, "t(\"", keys);
    extract_string_args_sq(&content, "t('", keys);

    Ok(())
}

/// Scan all .ts files in a directory for localizable strings.
fn scan_directory_for_keys(dir: &Path, keys: &mut BTreeSet<String>) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir()
            && !path
                .file_name()
                .is_some_and(|n| n == "node_modules" || n == "dist" || n == ".perry")
        {
            scan_directory_for_keys(&path, keys)?;
        } else if path
            .extension()
            .is_some_and(|ext| ext == "ts" || ext == "tsx")
        {
            scan_file_for_keys(&path, keys)?;
        }
    }
    Ok(())
}

/// Extract string arguments from double-quoted patterns like `Button("Next"`.
/// Handles escaped quotes: `Button("Hello, \"world\"")` extracts `Hello, "world"`.
fn extract_string_args(content: &str, prefix: &str, keys: &mut BTreeSet<String>) {
    let mut start = 0;
    while let Some(idx) = content[start..].find(prefix) {
        let abs = start + idx + prefix.len();
        if let Some(key) = find_closing_quote(&content[abs..], '"') {
            if !key.is_empty() && !key.contains('\n') {
                keys.insert(key);
            }
        }
        start = abs;
    }
}

/// Extract string arguments from single-quoted patterns like `Button('Next'`.
fn extract_string_args_sq(content: &str, prefix: &str, keys: &mut BTreeSet<String>) {
    let mut start = 0;
    while let Some(idx) = content[start..].find(prefix) {
        let abs = start + idx + prefix.len();
        if let Some(key) = find_closing_quote(&content[abs..], '\'') {
            if !key.is_empty() && !key.contains('\n') {
                keys.insert(key);
            }
        }
        start = abs;
    }
}

/// Find the unescaped closing quote and return the string content.
fn find_closing_quote(s: &str, quote: char) -> Option<String> {
    let mut result = String::new();
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            // Escaped character — consume the next char
            if let Some(next) = chars.next() {
                result.push(next);
            }
        } else if ch == quote {
            return Some(result);
        } else {
            result.push(ch);
        }
    }
    None
}
