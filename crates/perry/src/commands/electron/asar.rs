//! Minimal reader/extractor for Electron's `asar` archive format.
//!
//! Layout (all integers little-endian), validated byte-for-byte against a
//! real `npx @electron/asar pack` archive:
//!
//! ```text
//! u32 4                           (size of the leading size field itself)
//! u32 header_pickle_size          (4 + json_pickle_size; data starts at 8 + this)
//! u32 json_pickle_size            (4 + json_len + padding)
//! u32 json_len
//! u8  json[json_len]              {"files": {...}} tree
//! u8  padding                     (json pickle aligned to 4 bytes)
//! u8  data[...]                   file payloads, offsets relative to here
//! ```
//!
//! The JSON tree maps each directory name to `{"files": {...}}` and each
//! file to `{"size": <number>, "offset": "<decimal string>", ...}`. File
//! offsets are relative to the start of the data section, which is
//! `8 + header_pickle_size` from the archive start.

use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// One file entry flattened out of the header JSON tree.
struct FileEntry {
    /// Archive-relative path components (already traversal-validated).
    components: Vec<String>,
    /// Offset of the payload relative to the data-section start.
    offset: u64,
    size: u64,
    /// `unpacked: true` entries store their payload next to the archive in
    /// `<name>.unpacked/` rather than inside it.
    unpacked: bool,
}

/// Parse the archive header and return `(file_entries, data_section_start)`.
fn parse_header(archive: &[u8]) -> Result<(Vec<FileEntry>, u64)> {
    if archive.len() < 16 {
        bail!(
            "truncated asar archive: {} bytes, need at least 16",
            archive.len()
        );
    }
    let read_u32 = |at: usize| -> Result<u64> {
        let end = at
            .checked_add(4)
            .ok_or_else(|| anyhow!("asar offset overflow"))?;
        let bytes = archive
            .get(at..end)
            .ok_or_else(|| anyhow!("truncated asar header at offset {at}"))?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as u64)
    };

    let header_pickle_size = read_u32(4)?;
    let json_pickle_size = read_u32(8)?;
    let json_len = read_u32(12)?;
    // The header pickle occupies [8, 8 + header_pickle_size) and nests the
    // JSON pickle: [json_pickle_size][json_len][json][padding-to-4]. The
    // JSON pickle body starts at 12 (after its own size field), so its end
    // is 12 + json_pickle_size, which also marks the data section start
    // when the header pickle has no trailing slack.
    let pickle_end = 8u64
        .checked_add(header_pickle_size)
        .ok_or_else(|| anyhow!("asar header size overflow"))?;
    if pickle_end > archive.len() as u64 {
        bail!(
            "asar header size {header_pickle_size} exceeds archive length {}",
            archive.len()
        );
    }
    let json_pickle_end = 12u64
        .checked_add(json_pickle_size)
        .ok_or_else(|| anyhow!("asar json pickle size overflow"))?;
    if json_pickle_end > pickle_end {
        bail!("asar json pickle size {json_pickle_size} overflows the header pickle");
    }
    let json_end = 16u64
        .checked_add(json_len)
        .ok_or_else(|| anyhow!("asar json length overflow"))?;
    if json_end > json_pickle_end {
        bail!("asar json length {json_len} overflows the json pickle");
    }
    let json: Value = serde_json::from_slice(
        archive
            .get(16..json_end as usize)
            .ok_or_else(|| anyhow!("truncated asar header json"))?,
    )
    .context("invalid asar header json")?;

    let mut entries = Vec::new();
    let files = json
        .get("files")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow!("asar header json has no \"files\" object"))?;
    collect_entries(files, &mut Vec::new(), &mut entries)?;

    Ok((entries, pickle_end))
}

/// Recursively flatten the header tree, rejecting traversal / absolute /
/// separator-bearing names before they ever reach the filesystem.
fn collect_entries(
    dir: &serde_json::Map<String, Value>,
    prefix: &mut Vec<String>,
    out: &mut Vec<FileEntry>,
) -> Result<()> {
    for (name, node) in dir {
        validate_name(name)?;
        prefix.push(name.clone());
        if let Some(children) = node.get("files").and_then(Value::as_object) {
            collect_entries(children, prefix, out)?;
        } else {
            let size = node
                .get("size")
                .and_then(Value::as_u64)
                .ok_or_else(|| anyhow!("asar entry '{}' has no numeric size", prefix.join("/")))?;
            let offset = node
                .get("offset")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("asar entry '{}' has no string offset", prefix.join("/")))?
                .parse::<u64>()
                .with_context(|| {
                    format!("asar entry '{}' has a malformed offset", prefix.join("/"))
                })?;
            let unpacked = node
                .get("unpacked")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            out.push(FileEntry {
                components: prefix.clone(),
                offset,
                size,
                unpacked,
            });
        }
        prefix.pop();
    }
    Ok(())
}

/// Reject archive-entry names that could escape the extraction target:
/// empty, `.`/`..`, or anything carrying a path separator (the asar format
/// names one path component per JSON key, so a separator is always hostile).
fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() || name == "." || name == ".." {
        bail!("asar entry name '{name}' is not a safe path component");
    }
    if name.contains(['/', '\\']) || name.contains('\0') {
        bail!("asar entry name '{name}' contains a path separator or NUL");
    }
    Ok(())
}

/// Extract every file in the archive to `target` (created if missing).
///
/// Payload offsets and sizes are bounds-checked against the archive length
/// before any bytes are written; `unpacked: true` entries are copied from
/// the sibling `<archive-stem>.unpacked/` directory instead of the archive
/// body.
pub fn extract(asar_path: &Path, target: &Path) -> Result<()> {
    let archive = fs::read(asar_path)
        .with_context(|| format!("cannot read asar archive {}", asar_path.display()))?;
    let archive_len = archive.len() as u64;
    let (entries, data_start) = parse_header(&archive)?;
    let unpacked_dir = unpacked_dir(asar_path);

    fs::create_dir_all(target)
        .with_context(|| format!("cannot create extraction dir {}", target.display()))?;

    for entry in &entries {
        let mut out_path = target.to_path_buf();
        for component in &entry.components {
            out_path.push(component);
        }

        if entry.unpacked {
            let mut src = unpacked_dir.clone();
            for component in &entry.components {
                src.push(component);
            }
            if !src.is_file() {
                bail!(
                    "asar entry '{}' is marked unpacked but {} is missing",
                    entry.components.join("/"),
                    src.display()
                );
            }
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&src, &out_path)
                .with_context(|| format!("cannot copy unpacked entry from {}", src.display()))?;
            continue;
        }

        let start = data_start.checked_add(entry.offset).ok_or_else(|| {
            anyhow!(
                "asar entry '{}' offset overflow",
                entry.components.join("/")
            )
        })?;
        let end = start
            .checked_add(entry.size)
            .ok_or_else(|| anyhow!("asar entry '{}' size overflow", entry.components.join("/")))?;
        if end > archive_len {
            bail!(
                "asar entry '{}' (offset {}, size {}) extends past the archive \
                 length {archive_len} — the archive is truncated or malformed",
                entry.components.join("/"),
                entry.offset,
                entry.size
            );
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = fs::File::create(&out_path)
            .with_context(|| format!("cannot create {}", out_path.display()))?;
        out.write_all(archive.get(start as usize..end as usize).ok_or_else(|| {
            anyhow!(
                "asar entry '{}' payload out of bounds",
                entry.components.join("/")
            )
        })?)
        .with_context(|| format!("cannot write {}", out_path.display()))?;
    }
    Ok(())
}

/// Convenience: extract only if the cached copy is stale. `key` identifies
/// the source archive (path + length + mtime); when the marker file in
/// `target` still matches, the existing extraction is reused as-is.
pub fn extract_cached(asar_path: &Path, target: &Path, key: &str) -> Result<bool> {
    let marker = target.join(".perry-asar-source");
    if marker.is_file() && fs::read_to_string(&marker).ok().as_deref() == Some(key) {
        return Ok(false);
    }
    if target.exists() {
        fs::remove_dir_all(target)
            .with_context(|| format!("cannot refresh extraction dir {}", target.display()))?;
    }
    extract(asar_path, target)?;
    fs::write(&marker, key)?;
    Ok(true)
}

/// Path of the sibling unpacked-payload directory for an archive
/// (`app.asar` → `app.asar.unpacked`), whether or not it exists.
pub fn unpacked_dir(asar_path: &Path) -> PathBuf {
    asar_path.with_extension("asar.unpacked")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hand-craft an asar archive from a flat map of archive-relative paths
    /// to payload bytes. The byte layout here mirrors a real
    /// `npx @electron/asar pack` archive: u32le(4), u32le(header pickle
    /// size), u32le(json pickle size), u32le(json len), json, zero padding
    /// to a 4-byte boundary; the data section starts at
    /// `8 + header_pickle_size` and file offsets are relative to that point.
    fn build_archive(files: &[(&str, &[u8])]) -> Vec<u8> {
        // Build the nested {"files": ...} header tree, assigning data
        // offsets in the order given.
        let mut root = serde_json::Map::new();
        let mut offset = 0u64;
        for (path, bytes) in files {
            let components: Vec<&str> = path.split('/').collect();
            insert_file(&mut root, &components, offset, bytes.len());
            offset += bytes.len() as u64;
        }
        let json = serde_json::json!({ "files": root });
        let json_bytes = serde_json::to_vec(&json).unwrap();
        let padding = (4 - (json_bytes.len() % 4)) % 4;
        let json_pickle_size = 4 + json_bytes.len() + padding;
        let header_pickle_size = 4 + json_pickle_size;

        let mut archive = Vec::new();
        archive.extend_from_slice(&4u32.to_le_bytes());
        archive.extend_from_slice(&(header_pickle_size as u32).to_le_bytes());
        archive.extend_from_slice(&(json_pickle_size as u32).to_le_bytes());
        archive.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
        archive.extend_from_slice(&json_bytes);
        archive.extend(std::iter::repeat(0u8).take(padding));
        for (_, bytes) in files {
            archive.extend_from_slice(bytes);
        }
        archive
    }

    /// Insert one payload into the nested header tree at `components`.
    fn insert_file(
        tree: &mut serde_json::Map<String, Value>,
        components: &[&str],
        offset: u64,
        size: usize,
    ) {
        let (head, rest) = components.split_first().expect("at least one component");
        if rest.is_empty() {
            tree.insert(
                head.to_string(),
                serde_json::json!({
                    "size": size,
                    "offset": offset.to_string(),
                }),
            );
        } else {
            let dir = tree
                .entry(head.to_string())
                .or_insert_with(|| serde_json::json!({ "files": {} }));
            insert_file(
                dir.get_mut("files")
                    .expect("directory node")
                    .as_object_mut()
                    .expect("files object"),
                rest,
                offset,
                size,
            );
        }
    }

    #[test]
    fn extracts_nested_empty_and_unicode_entries() {
        let archive = build_archive(&[
            ("hello.txt", b"hello world"),
            ("empty.bin", b""),
            ("sub/nested.txt", b"nested"),
            ("füße-文件.txt", "uni".as_bytes()),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let asar = dir.path().join("fixture.asar");
        fs::write(&asar, &archive).unwrap();
        let out = dir.path().join("out");
        extract(&asar, &out).unwrap();

        assert_eq!(fs::read(out.join("hello.txt")).unwrap(), b"hello world");
        assert_eq!(fs::read(out.join("empty.bin")).unwrap(), b"");
        assert_eq!(fs::read(out.join("sub/nested.txt")).unwrap(), b"nested");
        assert_eq!(
            fs::read(out.join("füße-文件.txt")).unwrap(),
            "uni".as_bytes()
        );
    }

    #[test]
    fn rejects_traversal_entry_names() {
        for name in ["../evil.txt", "a/../../evil.txt", "..", ".", "a\\b.txt"] {
            let archive = build_archive(&[(name, b"x")]);
            let dir = tempfile::tempdir().unwrap();
            let asar = dir.path().join("fixture.asar");
            fs::write(&asar, &archive).unwrap();
            let err = extract(&asar, &dir.path().join("out")).unwrap_err();
            assert!(
                err.to_string().contains("not a safe path component")
                    || err.to_string().contains("path separator"),
                "unexpected error for {name:?}: {err}"
            );
        }
    }

    #[test]
    fn rejects_payload_extending_past_archive_end() {
        let json = serde_json::json!({ "files": { "big.bin": {
            "size": 9_000_000_000_000_000_000u64,
            "offset": "0",
        }}});
        let json_bytes = serde_json::to_vec(&json).unwrap();
        let padding = (4 - (json_bytes.len() % 4)) % 4;
        let json_pickle_size = 4 + json_bytes.len() + padding;
        let header_pickle_size = 4 + json_pickle_size;

        let mut archive = Vec::new();
        archive.extend_from_slice(&4u32.to_le_bytes());
        archive.extend_from_slice(&(header_pickle_size as u32).to_le_bytes());
        archive.extend_from_slice(&(json_pickle_size as u32).to_le_bytes());
        archive.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
        archive.extend_from_slice(&json_bytes);
        archive.extend(std::iter::repeat(0u8).take(padding));
        archive.extend_from_slice(b"short");

        let dir = tempfile::tempdir().unwrap();
        let asar = dir.path().join("fixture.asar");
        fs::write(&asar, &archive).unwrap();
        let err = extract(&asar, &dir.path().join("out")).unwrap_err();
        assert!(
            err.to_string().contains("extends past the archive"),
            "{err}"
        );
    }

    #[test]
    fn rejects_truncated_archive() {
        let archive = build_archive(&[("hello.txt", b"hello")]);
        let dir = tempfile::tempdir().unwrap();
        let asar = dir.path().join("fixture.asar");
        fs::write(&asar, &archive[..archive.len() - 3]).unwrap();
        let err = extract(&asar, &dir.path().join("out")).unwrap_err();
        assert!(err.to_string().contains("truncated"), "{err}");
    }

    #[test]
    fn cached_extract_skips_unchanged_archive() {
        let archive = build_archive(&[("hello.txt", b"hello")]);
        let dir = tempfile::tempdir().unwrap();
        let asar = dir.path().join("fixture.asar");
        fs::write(&asar, &archive).unwrap();
        let out = dir.path().join("out");

        assert!(extract_cached(&asar, &out, "key-v1").unwrap());
        assert!(!extract_cached(&asar, &out, "key-v1").unwrap());
        assert!(extract_cached(&asar, &out, "key-v2").unwrap());
        assert_eq!(fs::read(out.join("hello.txt")).unwrap(), b"hello");
    }
}
