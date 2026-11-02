//! Localizing the exports a kept wrapper member shares with its stdlib
//! replacement.
//!
//! The fixed-point shared-deps pass keeps a bundled codegen unit (`core`,
//! `alloc`, `std`, …) when a kept sibling needs a symbol only the wrapper's
//! copy defines. The stdlib's name-matched unit still has to load for the
//! symbols only IT defines, and both copies export the symbols they have in
//! common (same crate hash, same instantiation) — a hard `ld: duplicate
//! symbol` on Mach-O. Making the kept member's copy of the shared exports local
//! leaves the stdlib as their single provider; the symbols the wrapper
//! genuinely needs from its own copy stay global.

use super::*;

/// True when `path` is a Mach-O object (thin or fat, either endianness).
pub(super) fn object_is_macho(path: &Path) -> bool {
    use std::io::Read;
    let mut magic = [0u8; 4];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut magic))
        .map(|_| {
            matches!(u32::from_le_bytes(magic), 0xfeed_face | 0xfeed_facf)
                || matches!(
                    u32::from_be_bytes(magic),
                    0xfeed_face | 0xfeed_facf | 0xcafe_babe
                )
        })
        .unwrap_or(false)
}

/// For each kept candidate, localize the exports its stdlib replacement also
/// provides. Returns how many symbols were localized in total; a toolchain
/// without `llvm-objcopy` leaves the archive as is.
pub(super) fn localize_kept_shared_exports<'a>(
    nm: &Path,
    archive: &Path,
    extract_dir: &Path,
    kept: impl Iterator<Item = &'a String>,
    replacement_by_candidate: &std::collections::HashMap<String, std::collections::HashSet<String>>,
) -> Result<usize> {
    let Some(objcopy) = find_nightly_llvm_tool("llvm-objcopy")
        .or_else(|| find_llvm_tool("llvm-objcopy"))
        .or_else(|| find_path_tool("objcopy"))
    else {
        return Ok(0);
    };
    let exported_by_member =
        collect_archive_provided_symbols_by_member(nm, archive).ok_or_else(|| {
            anyhow::anyhow!("failed to inspect archive index of {}", archive.display())
        })?;
    let mut total = 0usize;
    for member in kept {
        let (Some(exported), Some(replacement)) = (
            exported_by_member.get(member),
            replacement_by_candidate.get(member),
        ) else {
            continue;
        };
        total +=
            localize_provided_by_replacement(&objcopy, extract_dir, member, exported, replacement)?;
    }
    Ok(total)
}

/// Localize, in the extracted Mach-O `member`, every extern symbol that
/// `replacement` also provides. Returns how many symbols were localized.
///
/// Expressed as a `--keep-global-symbols` keep-list: llvm-objcopy's
/// `--localize-symbol` leaves `LC_DYSYMTAB` inconsistent on Mach-O (see
/// [`dedup_ui_lib_against_linked_libs`]). Ring's C/assembly ABI stays global so
/// the Rust and native halves of a kept Ring unit never split across archives.
fn localize_provided_by_replacement(
    objcopy: &Path,
    extract_dir: &Path,
    member: &str,
    exported: &std::collections::HashSet<String>,
    replacement: &std::collections::HashSet<String>,
) -> Result<usize> {
    let Some(member_path) = extracted_archive_member(extract_dir, member) else {
        return Err(anyhow::anyhow!(
            "failed to locate extracted archive member {member}"
        ));
    };
    if !object_is_macho(&member_path) {
        return Ok(0);
    }
    let (shared, mut keep): (Vec<&String>, Vec<&String>) = exported
        .iter()
        .partition(|s| replacement.contains(*s) && !requires_bundled_native_companion(s));
    if shared.is_empty() {
        return Ok(0);
    }
    keep.sort();
    let list_path = extract_dir.join(format!("{member}.keep-globals-list"));
    std::fs::write(
        &list_path,
        keep.iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
    )?;
    let out = Command::new(objcopy)
        .arg(format!("--keep-global-symbols={}", list_path.display()))
        .arg(&member_path)
        .output()?;
    let _ = std::fs::remove_file(&list_path);
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(anyhow::anyhow!(
            "failed to localize shared exports in {member}: {stderr}"
        ));
    }
    Ok(shared.len())
}

#[cfg(test)]
mod tests;
