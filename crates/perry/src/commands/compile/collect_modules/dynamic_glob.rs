//! Dynamic-`import()` glob expansion (#1674 sub-part B). Split out of
//! `collect_modules.rs` to keep that file under the file-size gate.

/// #1674 sub-part B: expand a dynamic-`import()` glob pattern
/// (`<prefix>*<suffix>`, where `prefix` is a relative, directory-anchored
/// path) into concrete relative specifiers by reading the importing module's
/// directory. Each returned specifier equals the string the runtime template
/// produces (`prefix_dir + filename`), so the compile-time candidate keys match
/// the runtime dispatch arg exactly. Returns specifiers sorted for determinism.
pub(super) fn expand_dynamic_import_glob(
    importing_file: &str,
    prefix: &str,
    suffix: &str,
    cap: usize,
) -> Vec<String> {
    // Split the prefix into its directory part (through the last '/') and the
    // leading filename fragment that survivors must start with.
    let last_slash = match prefix.rfind('/') {
        Some(i) => i,
        None => return Vec::new(),
    };
    let prefix_dir = &prefix[..=last_slash]; // e.g. "./plugins/" or "./"
    let file_prefix = &prefix[last_slash + 1..]; // e.g. "" or "locale_"

    let importing_dir = std::path::Path::new(importing_file)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let glob_dir = importing_dir.join(prefix_dir);

    let entries = match std::fs::read_dir(&glob_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let min_len = file_prefix.len() + suffix.len();
    let mut out: Vec<String> = Vec::new();
    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        // The wildcard must match a non-empty middle: `name` strictly longer
        // than `file_prefix + suffix`, and bracketed by them.
        if name.len() <= min_len || !name.starts_with(file_prefix) || !name.ends_with(suffix) {
            continue;
        }
        let candidate = format!("{prefix_dir}{name}");
        if !out.contains(&candidate) {
            out.push(candidate);
        }
        if out.len() > cap {
            break;
        }
    }
    out.sort();
    out
}
