use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use super::super::strip_dedup::{find_nightly_llvm_tool, find_path_tool};
use super::find_llvm_tool;

const PREPARED_ARCHIVE_CACHE_VERSION: u32 = 1;
const PREPARED_ARCHIVE_CACHE_SCHEMA: &str = "perry-prepared-archives-v1";

#[derive(Debug, Serialize, Deserialize)]
struct PreparedArchiveManifest {
    version: u32,
    key: String,
    outputs: Vec<CachedOutput>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CachedOutput {
    hash: String,
    size: u64,
}

pub(super) struct PreparedArchiveInputs<'a> {
    pub cache_dir: &'a Path,
    pub target: Option<&'a str>,
    pub target_triple: Option<&'a str>,
    pub compiled_features: &'a [String],
    pub runtime_lib: &'a Path,
    pub stdlib_lib: Option<&'a Path>,
    pub well_known_libs: &'a [PathBuf],
}

pub(super) struct PreparedArchives {
    pub paths: Vec<PathBuf>,
    /// Cached entries are only written after every required archive transform
    /// succeeds. A false value means at least one original wrapper was kept
    /// as a non-fatal fallback and must not precede the full stdlib at link
    /// time: its bundled allocator/runtime objects could otherwise win.
    pub safe_to_precede_stdlib: bool,
}

/// Cache the expensive, deterministic strip/dedup pass over well-known native
/// archives. This is an intermediate-artifact cache: application objects and
/// linker flags remain covered by the final-link cache and are deliberately
/// absent here because they cannot affect the bytes produced by archive
/// preparation.
pub(super) fn prepare_well_known_archives<F>(
    inputs: PreparedArchiveInputs<'_>,
    prepare: F,
) -> PreparedArchives
where
    F: FnOnce() -> (Vec<PathBuf>, bool),
{
    if std::env::var_os("PERRY_NO_ARCHIVE_CACHE").is_some()
        || std::env::var("PERRY_NO_CACHE").as_deref() == Ok("1")
    {
        debug_cache("MISS", "disabled");
        let (paths, safe_to_precede_stdlib) = prepare();
        return PreparedArchives {
            paths,
            safe_to_precede_stdlib,
        };
    }

    let compiler = std::env::current_exe().ok();
    let tools = vec![
        find_llvm_tool("llvm-ar").or_else(|| find_path_tool("ar")),
        find_nightly_llvm_tool("llvm-nm")
            .or_else(|| find_llvm_tool("llvm-nm"))
            .or_else(|| find_path_tool("nm")),
        find_nightly_llvm_tool("llvm-objcopy")
            .or_else(|| find_llvm_tool("llvm-objcopy"))
            .or_else(|| find_path_tool("objcopy")),
    ];
    let key = match derive_prepared_archive_key(&inputs, compiler.as_deref(), &tools) {
        Ok(key) => key,
        Err(error) => {
            debug_cache("MISS", &format!("could not fingerprint inputs: {error}"));
            let (paths, safe_to_precede_stdlib) = prepare();
            return PreparedArchives {
                paths,
                safe_to_precede_stdlib,
            };
        }
    };
    let entry_dir = inputs
        .cache_dir
        .join("prepared-archives")
        .join(inputs.target.unwrap_or("native"))
        .join(&key);

    if let Some(paths) = load_cached_archives(&entry_dir, &key, inputs.well_known_libs) {
        debug_cache("HIT", &key);
        return PreparedArchives {
            paths,
            safe_to_precede_stdlib: true,
        };
    }

    debug_cache("MISS", &key);
    let (prepared, cacheable) = prepare();
    if !cacheable {
        debug_cache("MISS", "archive preparation used a non-fatal fallback");
        return PreparedArchives {
            paths: prepared,
            safe_to_precede_stdlib: false,
        };
    }
    let paths = match store_cached_archives(&entry_dir, &key, inputs.well_known_libs, &prepared) {
        Some(paths) => paths,
        None => prepared,
    };
    PreparedArchives {
        paths,
        safe_to_precede_stdlib: true,
    }
}

fn derive_prepared_archive_key(
    inputs: &PreparedArchiveInputs<'_>,
    compiler: Option<&Path>,
    tools: &[Option<PathBuf>],
) -> std::io::Result<String> {
    let mut hasher = Sha256::new();
    hash_field(&mut hasher, "schema", PREPARED_ARCHIVE_CACHE_SCHEMA);
    hash_field(&mut hasher, "perry-version", env!("CARGO_PKG_VERSION"));
    hash_field(&mut hasher, "target", inputs.target.unwrap_or("native"));
    hash_field(
        &mut hasher,
        "target-triple",
        inputs.target_triple.unwrap_or("native-host"),
    );
    if inputs.target_triple.is_none() {
        hash_field(&mut hasher, "host-arch", std::env::consts::ARCH);
        hash_field(&mut hasher, "host-os", std::env::consts::OS);
    }

    let mut features = inputs.compiled_features.to_vec();
    features.sort();
    features.dedup();
    for feature in features {
        hash_field(&mut hasher, "feature", &feature);
    }

    hash_optional_file(&mut hasher, "compiler", compiler)?;
    for (index, tool) in tools.iter().enumerate() {
        hash_field(&mut hasher, "tool-index", &index.to_string());
        hash_optional_file(&mut hasher, "tool", tool.as_deref())?;
    }
    hash_file(&mut hasher, "runtime", inputs.runtime_lib)?;
    hash_optional_file(&mut hasher, "stdlib", inputs.stdlib_lib)?;
    for (index, archive) in inputs.well_known_libs.iter().enumerate() {
        hash_field(&mut hasher, "archive-index", &index.to_string());
        hash_file(&mut hasher, "well-known", archive)?;
    }
    Ok(hex::encode(hasher.finalize()))
}

fn load_cached_archives(
    entry_dir: &Path,
    key: &str,
    source_archives: &[PathBuf],
) -> Option<Vec<PathBuf>> {
    let raw = fs::read(entry_dir.join("manifest.json")).ok()?;
    let manifest: PreparedArchiveManifest = serde_json::from_slice(&raw).ok()?;
    if manifest.version != PREPARED_ARCHIVE_CACHE_VERSION
        || manifest.key != key
        || manifest.outputs.len() != source_archives.len()
    {
        return None;
    }

    let mut paths = Vec::with_capacity(source_archives.len());
    for (index, (source, expected)) in source_archives.iter().zip(&manifest.outputs).enumerate() {
        let path = entry_dir.join(cached_archive_name(index, source));
        let (hash, size) = hash_file_value(&path).ok()?;
        if hash != expected.hash || size != expected.size {
            return None;
        }
        paths.push(path);
    }
    Some(paths)
}

fn store_cached_archives(
    entry_dir: &Path,
    key: &str,
    source_archives: &[PathBuf],
    prepared: &[PathBuf],
) -> Option<Vec<PathBuf>> {
    if prepared.len() != source_archives.len() || fs::create_dir_all(entry_dir).is_err() {
        return None;
    }

    let nonce = format!("{}-{}", std::process::id(), monotonic_nonce());
    let mut outputs = Vec::with_capacity(prepared.len());
    let mut cached_paths = Vec::with_capacity(prepared.len());
    for (index, (source, generated)) in source_archives.iter().zip(prepared).enumerate() {
        let destination = entry_dir.join(cached_archive_name(index, source));
        let temporary = destination.with_extension(format!("tmp-{nonce}"));
        if fs::copy(generated, &temporary).is_err() {
            let _ = fs::remove_file(&temporary);
            return None;
        }
        let (hash, size) = match hash_file_value(&temporary) {
            Ok(value) => value,
            Err(_) => {
                let _ = fs::remove_file(&temporary);
                return None;
            }
        };
        if destination.exists() {
            let _ = fs::remove_file(&destination);
        }
        if fs::rename(&temporary, &destination).is_err() {
            let _ = fs::remove_file(&temporary);
            return None;
        }
        outputs.push(CachedOutput { hash, size });
        cached_paths.push(destination);
    }

    let manifest = PreparedArchiveManifest {
        version: PREPARED_ARCHIVE_CACHE_VERSION,
        key: key.to_string(),
        outputs,
    };
    let bytes = serde_json::to_vec_pretty(&manifest).ok()?;
    let manifest_path = entry_dir.join("manifest.json");
    let temporary = entry_dir.join(format!("manifest.json.tmp-{nonce}"));
    if fs::write(&temporary, bytes).is_err() {
        return None;
    }
    if manifest_path.exists() {
        let _ = fs::remove_file(&manifest_path);
    }
    if fs::rename(&temporary, &manifest_path).is_err() {
        let _ = fs::remove_file(&temporary);
        return None;
    }
    Some(cached_paths)
}

fn cached_archive_name(index: usize, source: &Path) -> String {
    let extension = source
        .extension()
        .and_then(|value| value.to_str())
        .filter(|value| *value == "lib")
        .unwrap_or("a");
    format!("{index}.{extension}")
}

fn monotonic_nonce() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
}

fn debug_cache(result: &str, detail: &str) {
    if std::env::var("PERRY_CACHE_DEBUG_KEY").as_deref() == Ok("1") {
        eprintln!("[archive-cache] {result}: {detail}");
    }
}

fn hash_optional_file(
    hasher: &mut Sha256,
    label: &str,
    path: Option<&Path>,
) -> std::io::Result<()> {
    match path {
        Some(path) => hash_file(hasher, label, path),
        None => {
            hash_field(hasher, label, "<missing>");
            Ok(())
        }
    }
}

fn hash_file(hasher: &mut Sha256, label: &str, path: &Path) -> std::io::Result<()> {
    hash_field(hasher, &format!("{label}-name"), &path.to_string_lossy());
    let (hash, size) = hash_file_value(path)?;
    hash_field(hasher, &format!("{label}-size"), &size.to_string());
    hash_field(hasher, &format!("{label}-sha256"), &hash);
    Ok(())
}

fn hash_file_value(path: &Path) -> std::io::Result<(String, u64)> {
    let file = fs::File::open(path)?;
    let size = file.metadata()?.len();
    let mut reader = BufReader::with_capacity(256 * 1024, file);
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 256 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok((hex::encode(hasher.finalize()), size))
}

fn hash_field(hasher: &mut Sha256, name: &str, value: &str) {
    hasher.update((name.len() as u64).to_le_bytes());
    hasher.update(name.as_bytes());
    hasher.update((value.len() as u64).to_le_bytes());
    hasher.update(value.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(root: &Path, features: &[String], archives: &[PathBuf], target: &str) -> String {
        let runtime = root.join("runtime.a");
        let stdlib = root.join("stdlib.a");
        let inputs = PreparedArchiveInputs {
            cache_dir: root,
            target: Some(target),
            target_triple: Some(target),
            compiled_features: features,
            runtime_lib: &runtime,
            stdlib_lib: Some(&stdlib),
            well_known_libs: archives,
        };
        derive_prepared_archive_key(
            &inputs,
            Some(&root.join("perry")),
            &[
                Some(root.join("llvm-ar")),
                Some(root.join("llvm-nm")),
                Some(root.join("llvm-objcopy")),
            ],
        )
        .unwrap()
    }

    #[test]
    fn prepared_archive_key_covers_every_output_affecting_input() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        for (name, bytes) in [
            ("runtime.a", b"runtime-v1".as_slice()),
            ("stdlib.a", b"stdlib-v1".as_slice()),
            ("wrapper.a", b"wrapper-v1".as_slice()),
            ("perry", b"perry-v1".as_slice()),
            ("llvm-ar", b"ar-v1".as_slice()),
            ("llvm-nm", b"nm-v1".as_slice()),
            ("llvm-objcopy", b"objcopy-v1".as_slice()),
        ] {
            fs::write(root.join(name), bytes).unwrap();
        }
        let features = vec!["crypto".to_string(), "async-runtime".to_string()];
        let archives = vec![root.join("wrapper.a")];
        let baseline = key(root, &features, &archives, "macos");

        for (name, replacement) in [
            ("runtime.a", b"runtime-v2".as_slice()),
            ("stdlib.a", b"stdlib-v2".as_slice()),
            ("wrapper.a", b"wrapper-v2".as_slice()),
            ("perry", b"perry-v2".as_slice()),
            ("llvm-ar", b"ar-v2".as_slice()),
            ("llvm-nm", b"nm-v2".as_slice()),
            ("llvm-objcopy", b"objcopy-v2".as_slice()),
        ] {
            let path = root.join(name);
            let original = fs::read(&path).unwrap();
            fs::write(&path, replacement).unwrap();
            assert_ne!(baseline, key(root, &features, &archives, "macos"));
            fs::write(path, original).unwrap();
        }

        let changed_features = vec!["crypto".to_string(), "web-fetch".to_string()];
        assert_ne!(baseline, key(root, &changed_features, &archives, "macos"));
        assert_ne!(baseline, key(root, &features, &archives, "linux"));
    }

    #[test]
    fn prepared_archive_cache_rejects_corrupt_output() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        for name in ["runtime.a", "stdlib.a", "wrapper.a"] {
            fs::write(root.join(name), name).unwrap();
        }
        let archives = vec![root.join("wrapper.a")];
        let entry = root.join("entry");
        let key = "test-key";
        let prepared = root.join("prepared.a");
        fs::write(&prepared, b"prepared-v1").unwrap();
        let stored =
            store_cached_archives(&entry, key, &archives, std::slice::from_ref(&prepared)).unwrap();
        assert!(load_cached_archives(&entry, key, &archives).is_some());
        fs::write(&stored[0], b"corrupt").unwrap();
        assert!(load_cached_archives(&entry, key, &archives).is_none());
    }

    #[test]
    fn preparation_safety_survives_fallback_and_cache_hit() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let runtime = root.join("runtime.a");
        let stdlib = root.join("stdlib.a");
        let wrapper = root.join("wrapper.a");
        let prepared = root.join("prepared.a");
        for (path, bytes) in [
            (&runtime, b"runtime".as_slice()),
            (&stdlib, b"stdlib".as_slice()),
            (&wrapper, b"wrapper".as_slice()),
            (&prepared, b"prepared".as_slice()),
        ] {
            fs::write(path, bytes).unwrap();
        }
        let archives = vec![wrapper.clone()];
        let make_inputs = || PreparedArchiveInputs {
            cache_dir: root,
            target: Some("test-target"),
            target_triple: Some("test-target"),
            compiled_features: &[],
            runtime_lib: &runtime,
            stdlib_lib: Some(&stdlib),
            well_known_libs: &archives,
        };

        let fallback =
            prepare_well_known_archives(make_inputs(), || (vec![wrapper.clone()], false));
        assert_eq!(fallback.paths, vec![wrapper]);
        assert!(!fallback.safe_to_precede_stdlib);

        let stored = prepare_well_known_archives(make_inputs(), || (vec![prepared.clone()], true));
        assert!(stored.safe_to_precede_stdlib);
        assert_eq!(fs::read(&stored.paths[0]).unwrap(), b"prepared");

        let cached = prepare_well_known_archives(make_inputs(), || -> (Vec<PathBuf>, bool) {
            panic!("a valid cache hit must not prepare the archives again")
        });
        assert!(cached.safe_to_precede_stdlib);
        assert_eq!(cached.paths, stored.paths);
    }
}
