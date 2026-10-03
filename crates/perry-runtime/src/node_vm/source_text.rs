//! `node:vm` source-text helpers: the cached-data hash/bytes format and the
//! line-oriented module-source scanner (`parse_source` and its pieces).
//!
//! Split out of `node_vm.rs` to keep that file under the 2,000-line cap
//! (#10750); pure relocation.

use super::{ExportBinding, ImportBinding, ParsedSource, CACHE_PREFIX};

pub(super) fn source_hash(kind: u8, source: &str, params: &[String]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in [kind]
        .iter()
        .copied()
        .chain(source.as_bytes().iter().copied())
    {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    for param in params {
        hash ^= 0xff;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        for byte in param.as_bytes() {
            hash ^= *byte as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

pub(super) fn cached_data_bytes(kind: u8, hash: u64) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(CACHE_PREFIX.len() + 9);
    bytes.extend_from_slice(CACHE_PREFIX);
    bytes.push(kind);
    bytes.extend_from_slice(&hash.to_le_bytes());
    bytes
}

pub(super) fn cache_bytes_accepted(bytes: &[u8], kind: u8, hash: u64) -> bool {
    bytes == cached_data_bytes(kind, hash).as_slice()
}

pub(super) fn extract_source_map_url(source: &str) -> Option<String> {
    for line in source.lines().rev() {
        let trimmed = line.trim();
        let marker = if let Some(idx) = trimmed.find("sourceMappingURL=") {
            idx + "sourceMappingURL=".len()
        } else {
            continue;
        };
        let tail = trimmed[marker..].trim();
        let tail = tail.strip_suffix("*/").unwrap_or(tail).trim();
        if !tail.is_empty() {
            return Some(tail.to_string());
        }
    }
    None
}

pub(super) fn split_source_statements(source: &str) -> Vec<String> {
    source
        .split(';')
        .flat_map(|part| {
            let trimmed = part.trim();
            if trimmed.contains('\n') {
                trimmed
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            } else if trimmed.is_empty() {
                Vec::new()
            } else {
                vec![trimmed.to_string()]
            }
        })
        .collect()
}

fn extract_quoted(input: &str) -> Option<String> {
    let mut quote_start = None;
    let mut quote_byte = b'\0';
    for (idx, byte) in input.as_bytes().iter().copied().enumerate() {
        if byte == b'\'' || byte == b'"' {
            quote_start = Some(idx + 1);
            quote_byte = byte;
            break;
        }
    }
    let start = quote_start?;
    let rest = &input[start..];
    let end_rel = rest.as_bytes().iter().position(|b| *b == quote_byte)?;
    Some(rest[..end_rel].to_string())
}

fn parse_import_clause(stmt: &str, specifier: &str) -> Vec<ImportBinding> {
    let Some(open) = stmt.find('{') else {
        return Vec::new();
    };
    let Some(close_rel) = stmt[open + 1..].find('}') else {
        return Vec::new();
    };
    let close = open + 1 + close_rel;
    stmt[open + 1..close]
        .split(',')
        .filter_map(|part| {
            let part = part.trim();
            if part.is_empty() {
                return None;
            }
            let (imported, local) = if let Some(as_idx) = part.find(" as ") {
                (
                    part[..as_idx].trim().to_string(),
                    part[as_idx + 4..].trim().to_string(),
                )
            } else {
                (part.to_string(), part.to_string())
            };
            Some(ImportBinding {
                specifier: specifier.to_string(),
                imported,
                local,
            })
        })
        .collect()
}

fn parse_export_const(stmt: &str) -> Option<ExportBinding> {
    let prefixes = ["export const ", "export let ", "export var "];
    let body = prefixes
        .iter()
        .find_map(|prefix| stmt.strip_prefix(prefix))?;
    let eq = body.find('=')?;
    let name = body[..eq].trim();
    if name.is_empty() {
        return None;
    }
    Some(ExportBinding {
        name: name.to_string(),
        expr: body[eq + 1..].trim().to_string(),
    })
}

pub(super) fn parse_source(source: &str) -> ParsedSource {
    let mut requests = Vec::new();
    let mut imports = Vec::new();
    let mut exports = Vec::new();

    for stmt in split_source_statements(source) {
        if stmt.starts_with("import ") {
            if let Some(specifier) = stmt
                .find(" from ")
                .and_then(|idx| extract_quoted(&stmt[idx..]))
            {
                if !requests.iter().any(|existing| existing == &specifier) {
                    requests.push(specifier.clone());
                }
                imports.extend(parse_import_clause(&stmt, &specifier));
            } else if let Some(specifier) = extract_quoted(&stmt) {
                if !requests.iter().any(|existing| existing == &specifier) {
                    requests.push(specifier);
                }
            }
        } else if let Some(export) = parse_export_const(&stmt) {
            exports.push(export);
        }
    }

    ParsedSource {
        requests,
        imports,
        exports,
        has_top_level_await: source.contains("await "),
    }
}
