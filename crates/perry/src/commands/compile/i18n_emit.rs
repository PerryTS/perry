//! i18n emit helpers: Android `res/values-*/strings.xml`, the
//! `.perry/i18n-keys.json` registry, and Apple `<bundle>/<locale>.lproj/
//! Localizable.strings` writers.
//!
//! Extracted from `compile.rs` for issue #1105 PR 3 (directory split).
//! Pure file move — no behavior change. Callers in the orchestrator
//! and in `bundle_for_visionos` (and, later, the iOS / macOS bundle
//! writers) share these routines for emitting i18n artifacts next to
//! the linked binary.

use crate::OutputFormat;
use std::fs;
use std::path::Path;

use super::CompilationContext;

/// i18n: write the key registry to `.perry/i18n-keys.json` for tooling
/// (translators, key-extraction scripts) that needs a flat list of
/// emitted keys + their string-table indices. Best-effort.
pub(super) fn write_i18n_key_registry(
    ctx: &CompilationContext,
    i18n_table: Option<&perry_transform::i18n::I18nStringTable>,
) {
    let table = match i18n_table {
        Some(t) if !t.keys.is_empty() => t,
        _ => return,
    };
    let perry_dir = ctx.project_root.join(".perry");
    let _ = fs::create_dir_all(&perry_dir);
    let registry: Vec<serde_json::Value> = table
        .keys
        .iter()
        .enumerate()
        .map(|(i, key)| {
            serde_json::json!({
                "key": key,
                "string_idx": i,
            })
        })
        .collect();
    let registry_json = serde_json::json!({ "keys": registry });
    let _ = fs::write(
        perry_dir.join("i18n-keys.json"),
        serde_json::to_string_pretty(&registry_json).unwrap_or_default(),
    );
}

/// i18n: emit `res/values-<locale>/strings.xml` next to the Android
/// `.so` for every configured locale. Best-effort; failures don't
/// abort the build. Keys are sanitized to `[A-Za-z0-9_]+` (Android
/// resource-name grammar) and values get the usual XML entity escape.
pub(super) fn emit_android_i18n_resources(
    is_android: bool,
    i18n_table: Option<&perry_transform::i18n::I18nStringTable>,
    i18n_config: Option<&perry_transform::i18n::I18nConfig>,
    exe_path: &Path,
    format: OutputFormat,
) {
    if !is_android {
        return;
    }
    let (table, config) = match (i18n_table, i18n_config) {
        (Some(t), Some(c)) => (t, c),
        _ => return,
    };
    if table.keys.is_empty() {
        return;
    }
    let output_dir = exe_path.parent().unwrap_or(Path::new("."));
    let res_dir = output_dir.join("res");
    for (locale_idx, locale) in config.locales.iter().enumerate() {
        let values_dir = if locale_idx == 0 {
            res_dir.join("values") // default locale
        } else {
            res_dir.join(format!("values-{}", locale))
        };
        let _ = fs::create_dir_all(&values_dir);
        let mut xml = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<resources>\n");
        for (key_idx, key) in table.keys.iter().enumerate() {
            let flat_idx = locale_idx * table.keys.len() + key_idx;
            let value = table
                .translations
                .get(flat_idx)
                .cloned()
                .unwrap_or_else(|| key.clone());
            let res_name: String = key
                .chars()
                .map(|c| {
                    if c.is_alphanumeric() || c == '_' {
                        c
                    } else {
                        '_'
                    }
                })
                .collect();
            let escaped = value
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('"', "&quot;")
                .replace('\'', "\\'");
            xml.push_str(&format!(
                "    <string name=\"{}\">{}</string>\n",
                res_name, escaped
            ));
        }
        xml.push_str("</resources>\n");
        let _ = fs::write(values_dir.join("strings.xml"), &xml);
    }
    match format {
        OutputFormat::Text => println!(
            "  Generated res/values-*/strings.xml for {} locale(s)",
            config.locales.len()
        ),
        OutputFormat::Json => {}
    }
}

/// Write `<bundle>/<locale>.lproj/Localizable.strings` for every
/// configured locale. Used by the iOS / macOS / visionOS bundle
/// writers — Foundation `NSLocalizedString(key)` resolves from these
/// files when the user's preferred language matches the directory.
pub(super) fn write_lproj_localized_strings(
    app_dir: &Path,
    i18n_table: Option<&perry_transform::i18n::I18nStringTable>,
    i18n_config: Option<&perry_transform::i18n::I18nConfig>,
) {
    let (table, config) = match (i18n_table, i18n_config) {
        (Some(t), Some(c)) if !t.keys.is_empty() => (t, c),
        _ => return,
    };
    for (locale_idx, locale) in config.locales.iter().enumerate() {
        let lproj_dir = app_dir.join(format!("{}.lproj", locale));
        let _ = fs::create_dir_all(&lproj_dir);
        let mut strings_content = String::new();
        for (key_idx, key) in table.keys.iter().enumerate() {
            let flat_idx = locale_idx * table.keys.len() + key_idx;
            let value = table
                .translations
                .get(flat_idx)
                .cloned()
                .unwrap_or_else(|| key.clone());
            let escaped_key = key.replace('\\', "\\\\").replace('"', "\\\"");
            let escaped_val = value.replace('\\', "\\\\").replace('"', "\\\"");
            strings_content.push_str(&format!("\"{}\" = \"{}\";\n", escaped_key, escaped_val));
        }
        let _ = fs::write(lproj_dir.join("Localizable.strings"), &strings_content);
    }
}
