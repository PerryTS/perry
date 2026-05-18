//! `perry widget init <name>` — scaffold a WidgetKit (SwiftUI) source tree.
//!
//! Issue #676. Lays down a minimal WidgetKit boilerplate under
//! `ios-widgets/<name>/` (`<name>Widget.swift` with TimelineProvider +
//! WidgetEntryView, plus an AppIntent stub). A matching `[[widget]]` entry
//! is appended to `perry.toml` so the next `perry compile --target ios`
//! picks the widget up and embeds the produced `.appex` into the host
//! `.app/Frameworks/`.
//!
//! The scaffolder is iOS-only for v1. watchOS (`watchos_source`) and
//! Android Glance (`glance_source`) slots are accepted in the
//! `perry.toml` schema but skipped at compile time with a warning until
//! follow-up issues wire those build paths up.

use anyhow::{anyhow, Context, Result};
use clap::{Args, Subcommand};
use std::fs;
use std::path::{Path, PathBuf};

use crate::OutputFormat;

#[derive(Args, Debug)]
pub struct WidgetArgs {
    #[command(subcommand)]
    pub command: WidgetCommand,
}

#[derive(Subcommand, Debug)]
pub enum WidgetCommand {
    /// Scaffold a SwiftUI WidgetKit source tree under `ios-widgets/<name>/`.
    ///
    /// Generates `<name>Widget.swift` (Entry + TimelineProvider +
    /// WidgetEntryView + `@main` WidgetBundle) and `<name>Intent.swift`
    /// (AppIntent configuration stub). Appends a `[[widget]]` block to
    /// `perry.toml` so the next iOS compile picks the widget up.
    Init(InitArgs),
}

#[derive(Args, Debug)]
pub struct InitArgs {
    /// Widget name (PascalCase recommended — used as the Swift type name,
    /// the directory under `ios-widgets/`, and the `.appex` bundle name).
    pub name: String,

    /// Display name shown to the user in the widget gallery. Defaults to a
    /// title-cased version of `name`.
    #[arg(long)]
    pub display_name: Option<String>,

    /// One-line description shown beneath the display name in the gallery.
    #[arg(long)]
    pub description: Option<String>,

    /// Override the directory where the SwiftUI sources are written.
    /// Defaults to `ios-widgets/<name>`.
    #[arg(long)]
    pub dir: Option<PathBuf>,

    /// Skip appending a `[[widget]]` entry to `perry.toml`. Useful when
    /// scaffolding into a project that already has the entry, or for a
    /// dry run without manifest mutation.
    #[arg(long)]
    pub no_manifest_entry: bool,

    /// Overwrite the target directory if it already exists. Without this
    /// flag, `init` aborts to avoid clobbering hand-edited Swift sources.
    #[arg(long)]
    pub force: bool,
}

pub fn run(args: WidgetArgs, format: OutputFormat, _use_color: bool) -> Result<()> {
    match args.command {
        WidgetCommand::Init(a) => init_widget(a, format),
    }
}

fn init_widget(args: InitArgs, format: OutputFormat) -> Result<()> {
    let name = args.name.trim();
    if name.is_empty() {
        return Err(anyhow!("`perry widget init` requires a widget name"));
    }
    if !name
        .chars()
        .next()
        .map(|c| c.is_ascii_alphabetic())
        .unwrap_or(false)
        || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(anyhow!(
            "widget name must be a Swift-safe identifier (letters / digits / `_`, starts with a letter): got `{}`",
            name
        ));
    }

    let display_name = args
        .display_name
        .clone()
        .unwrap_or_else(|| humanize(name));
    let description = args
        .description
        .clone()
        .unwrap_or_else(|| format!("{} widget", display_name));

    let target_dir = args
        .dir
        .clone()
        .unwrap_or_else(|| PathBuf::from(format!("ios-widgets/{}", name)));

    if target_dir.exists() && !args.force {
        return Err(anyhow!(
            "directory `{}` already exists; pass --force to overwrite",
            target_dir.display()
        ));
    }
    fs::create_dir_all(&target_dir).with_context(|| {
        format!(
            "Failed to create widget source directory `{}`",
            target_dir.display()
        )
    })?;

    let widget_swift = render_widget_swift(name, &display_name, &description);
    let intent_swift = render_intent_swift(name, &display_name);

    let widget_path = target_dir.join(format!("{}Widget.swift", name));
    let intent_path = target_dir.join(format!("{}Intent.swift", name));
    fs::write(&widget_path, widget_swift)
        .with_context(|| format!("Failed to write `{}`", widget_path.display()))?;
    fs::write(&intent_path, intent_swift)
        .with_context(|| format!("Failed to write `{}`", intent_path.display()))?;

    let appended = if args.no_manifest_entry {
        false
    } else {
        append_widget_entry_to_perry_toml(name, &display_name, &description, &target_dir)?
    };

    match format {
        OutputFormat::Text => {
            println!("Scaffolded WidgetKit source tree at {}/", target_dir.display());
            println!("  {}", widget_path.display());
            println!("  {}", intent_path.display());
            if appended {
                println!("Appended [[widget]] entry to perry.toml.");
            } else if !args.no_manifest_entry {
                println!(
                    "Note: no perry.toml found in the working directory tree — \
                     add a [[widget]] entry manually so `perry compile --target ios` picks the widget up."
                );
            }
            println!();
            println!("Next steps:");
            println!("  1. Edit {} to point at your data / render shape.", widget_path.display());
            println!("  2. Run `perry compile --target ios` from your project root.");
            println!("     The widget builds via swiftc and lands at ");
            println!("     <output>.app/Frameworks/{}.appex/.", name);
        }
        OutputFormat::Json => {
            let result = serde_json::json!({
                "success": true,
                "name": name,
                "display_name": display_name,
                "directory": target_dir.to_string_lossy(),
                "files": [widget_path.to_string_lossy(), intent_path.to_string_lossy()],
                "perry_toml_appended": appended,
            });
            println!("{}", serde_json::to_string(&result)?);
        }
    }
    Ok(())
}

/// Insert spaces before runs of uppercase letters to humanize a PascalCase
/// identifier ("TopSitesWidget" → "Top Sites Widget").
fn humanize(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 4);
    for (i, ch) in name.chars().enumerate() {
        if i > 0 && ch.is_ascii_uppercase() {
            out.push(' ');
        }
        out.push(ch);
    }
    out
}

/// Render the per-widget SwiftUI source. Contains Entry, TimelineProvider,
/// EntryView, and a `@main` Widget struct so swiftc -emit-executable is
/// happy on its own. The user is expected to replace the placeholder
/// content (text body, sample data, timeline cadence) — the scaffold just
/// has to *compile*.
fn render_widget_swift(name: &str, display_name: &str, description: &str) -> String {
    format!(
        r#"// Auto-generated by `perry widget init {name}` — edit freely.
//
// Minimal SwiftUI WidgetKit boilerplate. `perry compile --target ios`
// picks this file up via the matching [[widget]] entry in perry.toml,
// invokes swiftc, and embeds the result at
// <output>.app/Frameworks/{name}.appex/.

import WidgetKit
import SwiftUI

struct {name}Entry: TimelineEntry {{
    let date: Date
    let title: String
}}

struct {name}Provider: TimelineProvider {{
    func placeholder(in context: Context) -> {name}Entry {{
        {name}Entry(date: Date(), title: "{display_name}")
    }}

    func getSnapshot(in context: Context, completion: @escaping ({name}Entry) -> ()) {{
        completion({name}Entry(date: Date(), title: "{display_name}"))
    }}

    func getTimeline(in context: Context, completion: @escaping (Timeline<{name}Entry>) -> ()) {{
        // Replace with your data fetch + cadence. Default refresh: every 30 min.
        let now = Date()
        let entry = {name}Entry(date: now, title: "{display_name}")
        let next = Calendar.current.date(byAdding: .minute, value: 30, to: now) ?? now
        completion(Timeline(entries: [entry], policy: .after(next)))
    }}
}}

struct {name}EntryView: View {{
    var entry: {name}Provider.Entry

    var body: some View {{
        VStack(alignment: .leading, spacing: 4) {{
            Text(entry.title)
                .font(.headline)
            Text(entry.date, style: .time)
                .font(.caption)
                .foregroundColor(.secondary)
        }}
        .padding()
    }}
}}

@main
struct {name}Widget: Widget {{
    let kind: String = "{name}"

    var body: some WidgetConfiguration {{
        StaticConfiguration(kind: kind, provider: {name}Provider()) {{ entry in
            {name}EntryView(entry: entry)
        }}
        .configurationDisplayName("{display_name}")
        .description("{description}")
        .supportedFamilies([.systemSmall, .systemMedium])
    }}
}}
"#,
        name = name,
        display_name = display_name,
        description = description,
    )
}

/// Render a per-widget AppIntent stub. Empty parameter list is enough to
/// satisfy the iOS 17 toolchain — users add `@Parameter` properties as
/// needed. Kept in its own file so the user can grow it without
/// scrolling past the timeline provider.
fn render_intent_swift(name: &str, display_name: &str) -> String {
    format!(
        r#"// Auto-generated by `perry widget init {name}` — edit freely.
//
// AppIntent stub for {name}. Replace the empty parameter list with the
// configurable properties you want users to pick from the widget gallery.

import AppIntents

struct {name}ConfigurationIntent: WidgetConfigurationIntent {{
    static var title: LocalizedStringResource = "Configure {display_name}"
    static var description = IntentDescription("Choose what {display_name} shows.")

    // Add `@Parameter` properties here, e.g.:
    //
    //     @Parameter(title: "Show preview", default: true)
    //     var showPreview: Bool
}}
"#,
        name = name,
        display_name = display_name,
    )
}

/// Append a `[[widget]]` block to the nearest `perry.toml` in the
/// working-directory tree. We don't try to merge with an existing entry —
/// if the user already declared the same widget, the second `[[widget]]`
/// block is the manifest-side equivalent of "the scaffolder did this
/// blind, the user should reconcile by hand". Returns whether an append
/// actually happened (false if no `perry.toml` exists).
fn append_widget_entry_to_perry_toml(
    name: &str,
    display_name: &str,
    description: &str,
    target_dir: &Path,
) -> Result<bool> {
    let perry_toml = match find_perry_toml(&std::env::current_dir()?) {
        Some(p) => p,
        None => return Ok(false),
    };
    let mut content = fs::read_to_string(&perry_toml)
        .with_context(|| format!("Failed to read `{}`", perry_toml.display()))?;
    // Use a relative path if `target_dir` is under the manifest's project
    // root; otherwise fall back to the absolute one we have. swiftc handles
    // either, but the manifest reads cleaner with the relative form.
    let project_root = perry_toml
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_default();
    let swift_source = if let Ok(abs) = target_dir.canonicalize() {
        abs.strip_prefix(&project_root)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| target_dir.to_string_lossy().to_string())
    } else {
        target_dir.to_string_lossy().to_string()
    };

    if !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(&format!(
        "\n[[widget]]\nname = \"{}\"\nswift_source = \"{}\"\ndisplay_name = \"{}\"\ndescription = \"{}\"\n",
        name, swift_source, display_name, description
    ));
    fs::write(&perry_toml, content)
        .with_context(|| format!("Failed to write `{}`", perry_toml.display()))?;
    Ok(true)
}

/// Walk up the working-directory chain looking for `perry.toml`. Stops
/// after 8 hops so a stray invocation outside any project doesn't crawl
/// the whole filesystem.
fn find_perry_toml(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    for _ in 0..8 {
        let candidate = dir.join("perry.toml");
        if candidate.exists() {
            return Some(candidate);
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn humanize_pascal_case() {
        assert_eq!(humanize("TopSitesWidget"), "Top Sites Widget");
        assert_eq!(humanize("Daily"), "Daily");
        assert_eq!(humanize("A"), "A");
    }

    #[test]
    fn render_widget_uses_name_everywhere() {
        let s = render_widget_swift("Demo", "Demo Widget", "A demo.");
        assert!(s.contains("struct DemoWidget: Widget"));
        assert!(s.contains("let kind: String = \"Demo\""));
        assert!(s.contains(".configurationDisplayName(\"Demo Widget\")"));
    }
}
