//! Lossless Bun asset storage, using its already-selected zstd runtime pack.
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

// Avoid pulling a decoder into a tiny asset-only program for a handful of
// saved bytes. This is a storage threshold, not a total-executable size claim.
const MIN_TOTAL_SAVING: u64 = 256 * 1024;
const MIN_ASSET_BYTES: u64 = 4096;

pub(super) struct Payload {
    pub(super) path: PathBuf,
    pub(super) original_len: Option<usize>,
}

pub(super) fn prepare(
    assets: &[(String, PathBuf)],
    output_dir: &Path,
    enabled: bool,
) -> Result<Vec<Payload>> {
    let mut payloads = Vec::with_capacity(assets.len());
    let mut saving = 0u64;
    for (idx, (_, source)) in assets.iter().enumerate() {
        let mut payload = Payload {
            path: source.clone(),
            original_len: None,
        };
        if enabled && fs::metadata(source)?.len() >= MIN_ASSET_BYTES {
            let bytes =
                fs::read(source).with_context(|| format!("read asset {}", source.display()))?;
            let mut compressor = zstd::bulk::Compressor::new(3)?;
            compressor.include_checksum(true)?;
            let packed = compressor
                .compress(&bytes)
                .context("compress embedded asset")?;
            if packed.len().saturating_add(64) < bytes.len() {
                let path = output_dir.join(format!("__perry_asset_{idx}.zst"));
                fs::write(&path, &packed)?;
                saving = saving.saturating_add((bytes.len() - packed.len()) as u64);
                payload = Payload {
                    path,
                    original_len: Some(bytes.len()),
                };
            }
        }
        payloads.push(payload);
    }
    if saving < MIN_TOTAL_SAVING {
        // Keep every original raw symbol/path on the small-payload arm.
        return Ok(assets
            .iter()
            .map(|(_, path)| Payload {
                path: path.clone(),
                original_len: None,
            })
            .collect());
    }
    Ok(payloads)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compresses_only_with_bun_and_a_material_aggregate_saving() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("large.bin");
        let bytes: Vec<u8> = (0..524288).map(|i| (i % 256) as u8).collect();
        fs::write(&source, &bytes).unwrap();
        let assets = vec![("large.bin".into(), source.clone())];
        let disabled = prepare(&assets, dir.path(), false).unwrap();
        assert!(disabled[0].original_len.is_none());
        assert_eq!(disabled[0].path, source);
        let enabled = prepare(&assets, dir.path(), true).unwrap();
        assert_eq!(enabled[0].original_len, Some(bytes.len()));
        let packed = fs::read(&enabled[0].path).unwrap();
        assert!(packed.len() < bytes.len() / 2);
        assert_eq!(zstd::bulk::decompress(&packed, bytes.len()).unwrap(), bytes);
        let mut damaged = packed;
        *damaged.last_mut().unwrap() ^= 1;
        assert!(zstd::bulk::decompress(&damaged, bytes.len()).is_err());
        assert_eq!(fs::read(&source).unwrap(), bytes);
    }

    #[test]
    fn small_assets_and_small_total_savings_keep_raw_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let tiny = dir.path().join("tiny.txt");
        let modest = dir.path().join("modest.txt");
        fs::write(&tiny, b"hello").unwrap();
        fs::write(&modest, vec![b'x'; 16384]).unwrap();
        let assets = vec![
            ("tiny.txt".into(), tiny.clone()),
            ("modest.txt".into(), modest.clone()),
        ];
        let result = prepare(&assets, dir.path(), true).unwrap();
        assert!(result.iter().all(|p| p.original_len.is_none()));
        assert_eq!(result[0].path, tiny);
        assert_eq!(result[1].path, modest);
    }

    #[test]
    fn incompressible_assets_remain_raw_beside_compressed_ones() {
        let dir = tempfile::tempdir().unwrap();
        let compressible = dir.path().join("compressible.bin");
        let raw = dir.path().join("random.bin");
        fs::write(&compressible, vec![0u8; 524288]).unwrap();
        let mut state = 0x1234_5678u32;
        let random: Vec<u8> = (0..524288)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state as u8
            })
            .collect();
        fs::write(&raw, &random).unwrap();
        let result = prepare(
            &[("a".into(), compressible), ("b".into(), raw.clone())],
            dir.path(),
            true,
        )
        .unwrap();
        assert!(result[0].original_len.is_some());
        assert!(result[1].original_len.is_none());
        assert_eq!(result[1].path, raw);
        assert_eq!(fs::read(raw).unwrap(), random);
    }
}
