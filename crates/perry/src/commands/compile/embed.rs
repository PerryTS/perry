//! Embed static assets/files into the standalone executable (#5731).
//!
//! Patterns come from three sources, unioned: the `--embed` CLI flag,
//! `perry.embed` in package.json, and `[compile] embed` in perry.toml. Each
//! pattern is a file, a directory (embedded recursively), or a `*`/`**` glob,
//! resolved relative to the project root. The matched files become
//! `(name, abs_path)` pairs where `name` is the project-root-relative path with
//! `/` separators (e.g. `dist/index.html`) — the runtime registry key and the
//! `$perryfs/<name>` virtual-path suffix.
//!
//! [`generate_embedded_asset_object`] emits a C source whose
//! `__attribute__((constructor))` calls `js_register_embedded_asset` once per
//! file before `main` runs (mirroring the embedded-JS object generator), then
//! compiles it to a `.o` that the caller appends to the link line. The bytes
//! land in the binary's read-only data as C string literals; the runtime keeps
//! `&'static` slices into them (no copy).

use anyhow::{anyhow, Result};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Collect the embed patterns from the CLI flag plus `perry.embed`
/// (package.json) and `[compile] embed` (perry.toml) under `project_root`,
/// expand them against `project_root`, and return de-duplicated, sorted
/// `(name, absolute_path)` pairs.
pub(super) fn resolve_embedded_assets(
    cli_patterns: &[String],
    project_root: &Path,
) -> Result<Vec<(String, PathBuf)>> {
    let mut patterns: Vec<String> = cli_patterns.to_vec();
    patterns.extend(read_package_json_embed(project_root));
    patterns.extend(read_perry_toml_embed(project_root));

    let mut assets: Vec<(String, PathBuf)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for pattern in &patterns {
        for path in expand_pattern(pattern, project_root)? {
            let name = match relative_name(&path, project_root) {
                Some(n) => n,
                None => continue,
            };
            if seen.insert(name.clone()) {
                assets.push((name, path));
            }
        }
    }
    assets.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(assets)
}

/// `perry.embed` from `<project_root>/package.json` (array of strings).
fn read_package_json_embed(project_root: &Path) -> Vec<String> {
    let path = project_root.join("package.json");
    let Ok(text) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    json.get("perry")
        .and_then(|p| p.get("embed"))
        .and_then(|e| e.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// `[compile] embed` from `<project_root>/perry.toml` (array of strings).
fn read_perry_toml_embed(project_root: &Path) -> Vec<String> {
    let path = project_root.join("perry.toml");
    let Ok(text) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    let Ok(toml) = text.parse::<toml::Value>() else {
        return Vec::new();
    };
    toml.get("compile")
        .and_then(|c| c.get("embed"))
        .and_then(|e| e.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Expand one pattern (file / directory / glob) into a list of concrete files.
fn expand_pattern(pattern: &str, project_root: &Path) -> Result<Vec<PathBuf>> {
    let trimmed = pattern.trim_start_matches("./");
    let abs = if Path::new(pattern).is_absolute() {
        PathBuf::from(pattern)
    } else {
        project_root.join(trimmed)
    };

    // Plain path (no wildcards): a file embeds itself; a directory embeds all
    // files beneath it.
    if !has_wildcard(pattern) {
        if abs.is_file() {
            return Ok(vec![abs]);
        }
        if abs.is_dir() {
            return Ok(walk_files(&abs));
        }
        // Missing path — not fatal; skip with no match (a glob that matches
        // nothing is also non-fatal). Caller-visible "embedded N files" makes
        // an empty result obvious.
        return Ok(Vec::new());
    }

    // Glob: split into the longest wildcard-free base dir and the wildcard
    // remainder, walk the base, and match each file's relative segments.
    let (base, rest) = split_glob_base(trimmed);
    let base_dir = project_root.join(&base);
    if !base_dir.is_dir() {
        return Ok(Vec::new());
    }
    let pat_segments: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
    let mut out = Vec::new();
    for file in walk_files(&base_dir) {
        let Ok(rel) = file.strip_prefix(&base_dir) else {
            continue;
        };
        let rel_segments: Vec<String> = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        let rel_refs: Vec<&str> = rel_segments.iter().map(String::as_str).collect();
        if glob_match(&pat_segments, &rel_refs) {
            out.push(file);
        }
    }
    Ok(out)
}

fn has_wildcard(s: &str) -> bool {
    s.contains('*') || s.contains('?')
}

/// Split a glob pattern into `(base_without_wildcards, remainder)`. e.g.
/// `dist/assets/**/*.png` → (`dist/assets`, `**/*.png`).
fn split_glob_base(pattern: &str) -> (String, String) {
    let segments: Vec<&str> = pattern.split('/').collect();
    let mut base = Vec::new();
    let mut idx = 0;
    while idx < segments.len() && !has_wildcard(segments[idx]) {
        base.push(segments[idx]);
        idx += 1;
    }
    let rest: Vec<&str> = segments[idx..].to_vec();
    (base.join("/"), rest.join("/"))
}

/// Recursively collect all regular files under `dir`, sorted for determinism.
fn walk_files(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = walkdir::WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .map(|e| e.into_path())
        .collect();
    out.sort();
    out
}

/// Match glob pattern segments against path segments. `**` matches zero or more
/// path segments; within a segment, `*` matches any run of non-`/` chars and
/// `?` matches a single char.
fn glob_match(pat: &[&str], path: &[&str]) -> bool {
    match pat.split_first() {
        None => path.is_empty(),
        Some((&"**", rest)) => {
            // `**` consumes 0..=path.len() leading segments.
            (0..=path.len()).any(|i| glob_match(rest, &path[i..]))
        }
        Some((seg, rest)) => match path.split_first() {
            Some((first, prest)) if segment_match(seg, first) => glob_match(rest, prest),
            _ => false,
        },
    }
}

/// Match a single path segment against a single pattern segment containing
/// `*` / `?` wildcards.
fn segment_match(pat: &str, text: &str) -> bool {
    let p: Vec<char> = pat.chars().collect();
    let t: Vec<char> = text.chars().collect();
    fn rec(p: &[char], t: &[char]) -> bool {
        match p.split_first() {
            None => t.is_empty(),
            Some(('*', prest)) => (0..=t.len()).any(|i| rec(prest, &t[i..])),
            Some(('?', prest)) => !t.is_empty() && rec(prest, &t[1..]),
            Some((c, prest)) => match t.split_first() {
                Some((tc, trest)) if tc == c => rec(prest, trest),
                _ => false,
            },
        }
    }
    rec(&p, &t)
}

/// Project-root-relative name with `/` separators, e.g. `dist/index.html`.
fn relative_name(path: &Path, project_root: &Path) -> Option<String> {
    let rel = path.strip_prefix(project_root).ok()?;
    let name = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// Emit and compile the embedded-asset registration object. Returns `Ok(None)`
/// when there are no assets. The `.o` calls `js_register_embedded_asset` (a
/// perry-runtime symbol) once per file from a startup constructor.
pub(super) fn generate_embedded_asset_object(
    assets: &[(String, PathBuf)],
    output_dir: &Path,
) -> Result<Option<PathBuf>> {
    if assets.is_empty() {
        return Ok(None);
    }
    let c_path = output_dir.join("__perry_embedded_assets.c");
    let obj_path = output_dir.join("__perry_embedded_assets.o");

    let mut c = String::new();
    c.push_str("// Auto-generated by Perry — embedded asset table (#5731).\n");
    c.push_str("// A startup constructor registers each file's bytes into the\n");
    c.push_str("// runtime registry, readable via `perry` / node:fs at runtime.\n");
    c.push_str("#include <stddef.h>\n\n");
    c.push_str("extern void js_register_embedded_asset(const char *name, size_t name_len, const char *bytes, size_t bytes_len);\n\n");

    for (idx, (name, path)) in assets.iter().enumerate() {
        let bytes = fs::read(path)
            .map_err(|e| anyhow!("failed to read embed asset {}: {}", path.display(), e))?;
        let name_lit = c_byte_literal(name.as_bytes());
        let data_lit = c_byte_literal(&bytes);
        writeln!(c, "static const char PERRY_ASSET_NAME_{idx}[] = {name_lit};").ok();
        writeln!(
            c,
            "static const size_t PERRY_ASSET_NAME_LEN_{idx} = {};",
            name.len()
        )
        .ok();
        writeln!(c, "static const char PERRY_ASSET_DATA_{idx}[] = {data_lit};").ok();
        writeln!(
            c,
            "static const size_t PERRY_ASSET_DATA_LEN_{idx} = {};",
            bytes.len()
        )
        .ok();
    }

    // Constructor priority 101: runs before `main`'s `js_runtime_init`, so the
    // registry is populated by the time any user code or fs read consults it.
    c.push_str("__attribute__((constructor(101)))\n");
    c.push_str("static void perry_register_embedded_assets(void) {\n");
    for idx in 0..assets.len() {
        writeln!(
            c,
            "    js_register_embedded_asset(PERRY_ASSET_NAME_{idx}, PERRY_ASSET_NAME_LEN_{idx}, PERRY_ASSET_DATA_{idx}, PERRY_ASSET_DATA_LEN_{idx});"
        )
        .ok();
    }
    c.push_str("}\n");

    fs::write(&c_path, &c)?;

    let status = Command::new("cc")
        .arg("-c")
        .arg(&c_path)
        .arg("-O0")
        .arg("-o")
        .arg(&obj_path)
        .status()
        .map_err(|e| anyhow!("failed to invoke cc for embedded assets: {}", e))?;
    if !status.success() {
        return Err(anyhow!(
            "cc failed to compile embedded asset table ({})",
            c_path.display()
        ));
    }
    Ok(Some(obj_path))
}

/// Render arbitrary bytes as a C string literal. Printable ASCII passes
/// through; quotes/backslash/control bytes and everything ≥0x80 use octal
/// escapes, keeping the generated source ASCII-clean and binary-safe (the
/// length is tracked separately, so embedded NULs are fine). Mirrors the
/// `c_string_literal` helper in `targets.rs`.
fn c_byte_literal(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() + 2);
    out.push('"');
    for &b in bytes {
        match b {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            b'?' => out.push_str("\\?"),
            0x20..=0x7E => out.push(b as char),
            _ => {
                let _ = write!(out, "\\{:03o}", b);
            }
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(s: &str) -> Vec<&str> {
        s.split('/').filter(|x| !x.is_empty()).collect()
    }

    #[test]
    fn double_star_matches_any_depth() {
        assert!(glob_match(&seg("**"), &seg("index.html")));
        assert!(glob_match(&seg("**"), &seg("assets/app.js")));
        assert!(glob_match(&seg("**"), &seg("a/b/c/d.png")));
        assert!(glob_match(&seg("**"), &[])); // ** matches zero segments
    }

    #[test]
    fn double_star_with_extension_filter() {
        let p = seg("**/*.png");
        assert!(glob_match(&p, &seg("logo.png")));
        assert!(glob_match(&p, &seg("assets/logo.png")));
        assert!(glob_match(&p, &seg("a/b/logo.png")));
        assert!(!glob_match(&p, &seg("logo.jpg")));
        assert!(!glob_match(&p, &seg("assets/app.js")));
    }

    #[test]
    fn single_star_is_one_segment_only() {
        let p = seg("*.css");
        assert!(glob_match(&p, &seg("main.css")));
        assert!(!glob_match(&p, &seg("nested/main.css")));
    }

    #[test]
    fn question_mark_matches_single_char() {
        assert!(segment_match("a?c", "abc"));
        assert!(!segment_match("a?c", "ac"));
        assert!(!segment_match("a?c", "abbc"));
    }

    #[test]
    fn split_glob_base_separates_static_prefix() {
        assert_eq!(
            split_glob_base("dist/assets/**/*.png"),
            ("dist/assets".to_string(), "**/*.png".to_string())
        );
        assert_eq!(
            split_glob_base("dist/**"),
            ("dist".to_string(), "**".to_string())
        );
        assert_eq!(
            split_glob_base("*.html"),
            (String::new(), "*.html".to_string())
        );
    }

    #[test]
    fn expand_directory_and_glob_resolve_relative_names() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("dist/assets")).unwrap();
        fs::write(root.join("dist/index.html"), b"<html>").unwrap();
        fs::write(root.join("dist/assets/app.js"), b"1").unwrap();
        fs::write(root.join("dist/assets/logo.png"), b"PNG").unwrap();

        // Whole directory.
        let all = resolve_embedded_assets(&["./dist".into()], root).unwrap();
        let names: Vec<&str> = all.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            vec!["dist/assets/app.js", "dist/assets/logo.png", "dist/index.html"]
        );

        // Glob with extension filter.
        let pngs = resolve_embedded_assets(&["./dist/**/*.png".into()], root).unwrap();
        let names: Vec<&str> = pngs.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["dist/assets/logo.png"]);

        // De-dup across overlapping patterns.
        let merged =
            resolve_embedded_assets(&["./dist/**".into(), "./dist/index.html".into()], root)
                .unwrap();
        assert_eq!(merged.len(), 3);
    }

    #[test]
    fn binary_literal_is_ascii_clean_and_escapes_specials() {
        let lit = c_byte_literal(&[0x00, b'"', b'\\', 0x41, 0xFF]);
        assert_eq!(lit, "\"\\000\\\"\\\\A\\377\"");
        assert!(lit.is_ascii());
    }
}
