//! Dynamic-`import()` glob expansion (#1674 sub-part B). Split out of
//! `collect_modules.rs` to keep that file under the file-size gate.

/// What a glob found: the specifiers a runtime template can produce, the
/// files that were enumerated to produce them, and the number of directory
/// entries that matched (the quantity the candidate cap limits).
pub(super) struct GlobMatches {
    pub specifiers: Vec<String>,
    pub files: Vec<std::path::PathBuf>,
    pub entries: usize,
}

/// Node's `require` extension probes, in its order (`Module._extensions`).
const REQUIRE_EXTENSIONS: [&str; 3] = [".js", ".json", ".node"];

/// A directory node can `require` by name: it has a `package.json` or an index.
fn is_requirable_dir(dir: &std::path::Path) -> bool {
    dir.join("package.json").is_file()
        || REQUIRE_EXTENSIONS
            .iter()
            .any(|ext| dir.join(format!("index{ext}")).is_file())
}

/// #1674 sub-part B: expand a dynamic-`import()` glob pattern
/// (`<prefix>*<suffix>`, where `prefix` is a relative, directory-anchored
/// path) into concrete relative specifiers by reading the importing module's
/// directory. Each returned specifier equals the string the runtime template
/// produces (`prefix_dir + filename`), so the compile-time candidate keys match
/// the runtime dispatch arg exactly. Returns specifiers sorted for determinism.
///
/// `node_require` makes it `require`'s resolution instead of `import()`'s: the
/// runtime string may also omit `.js`/`.json`/`.node`, or name a directory that
/// holds a `package.json` or an index file; each such spelling is a specifier
/// too, and the ordinary resolver maps it to its file.
pub(super) fn expand_dynamic_import_glob(
    importing_file: &str,
    prefix: &str,
    suffix: &str,
    cap: usize,
    node_require: bool,
) -> GlobMatches {
    let mut found = GlobMatches {
        specifiers: Vec::new(),
        files: Vec::new(),
        entries: 0,
    };
    // Split the prefix into its directory part (through the last '/') and the
    // leading filename fragment that survivors must start with.
    let last_slash = match prefix.rfind('/') {
        Some(i) => i,
        None => return found,
    };
    let prefix_dir = &prefix[..=last_slash]; // e.g. "./plugins/" or "./"
    let file_prefix = &prefix[last_slash + 1..]; // e.g. "" or "locale_"

    let importing_dir = std::path::Path::new(importing_file)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let glob_dir = importing_dir.join(prefix_dir);

    let entries = match std::fs::read_dir(&glob_dir) {
        Ok(e) => e,
        Err(_) => return found,
    };
    let min_len = file_prefix.len() + suffix.len();
    // The wildcard must match a non-empty middle: the spelling strictly longer
    // than `file_prefix + suffix`, and bracketed by them.
    let matches = |spelling: &str| {
        spelling.len() > min_len && spelling.starts_with(file_prefix) && spelling.ends_with(suffix)
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_file = path.is_file();
        if !is_file && !(node_require && path.is_dir()) {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let mut spellings: Vec<&str> = Vec::new();
        if is_file {
            spellings.push(name);
            if node_require {
                spellings.extend(
                    REQUIRE_EXTENSIONS
                        .iter()
                        .filter_map(|ext| name.strip_suffix(ext)),
                );
            }
        } else if is_requirable_dir(&path) {
            spellings.push(name);
        }
        let mut matched = false;
        for spelling in spellings.into_iter().filter(|s| matches(s)) {
            matched = true;
            let candidate = format!("{prefix_dir}{spelling}");
            if !found.specifiers.contains(&candidate) {
                found.specifiers.push(candidate);
            }
        }
        if matched {
            found.entries += 1;
            if is_file {
                found.files.push(path);
            }
        }
        if found.entries > cap {
            break;
        }
    }
    found.specifiers.sort();
    found
}

/// A glob candidate that does not parse is not a compile error: node fails only
/// when a `require` actually loads the file, with a `SyntaxError`. Returns the
/// source of a module that throws that error when it is initialized, or `None`
/// when the file is not a glob candidate or parses.
pub(super) fn unparsable_candidate_stub(
    is_glob_candidate: bool,
    source: &str,
    filename: &str,
) -> Option<String> {
    if !is_glob_candidate {
        return None;
    }
    let error = perry_parser::parse_typescript(source, filename).err()?;
    let message = error.to_string();
    let message = message.lines().next().unwrap_or_default();
    let message = serde_json::to_string(message).ok()?;
    Some(format!("throw new SyntaxError({message});\n"))
}
