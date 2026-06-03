use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::super::LinkCacheStats;

const LINK_CACHE_MANIFEST_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub(in crate::commands::compile) struct LinkCacheStatus {
    pub(super) linked: bool,
    state: Option<LinkCacheState>,
}

impl LinkCacheStatus {
    fn linked(state: Option<LinkCacheState>) -> Self {
        Self {
            linked: true,
            state,
        }
    }

    fn skipped(state: LinkCacheState) -> Self {
        Self {
            linked: false,
            state: Some(state),
        }
    }

    pub(in crate::commands::compile) fn stats(&self) -> LinkCacheStats {
        LinkCacheStats {
            linked: self.linked,
            skipped: !self.linked,
        }
    }
}

#[derive(Debug, Clone)]
struct LinkCacheState {
    manifest_path: PathBuf,
    link_fingerprint: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct LinkCacheManifest {
    version: u32,
    link_fingerprint: String,
    output_hash: String,
    output_size: u64,
}

pub(in crate::commands::compile) fn write_link_cache_manifest(
    status: &LinkCacheStatus,
    exe_path: &Path,
) {
    let Some(state) = status.state.as_ref() else {
        return;
    };
    let Ok((output_hash, output_size)) = hash_file_with_size(exe_path) else {
        return;
    };
    let manifest = LinkCacheManifest {
        version: LINK_CACHE_MANIFEST_VERSION,
        link_fingerprint: state.link_fingerprint.clone(),
        output_hash,
        output_size,
    };
    let Some(parent) = state.manifest_path.parent() else {
        return;
    };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    let Ok(bytes) = serde_json::to_vec_pretty(&manifest) else {
        return;
    };
    let tmp = state.manifest_path.with_extension("json.tmp");
    if fs::write(&tmp, bytes).is_ok() {
        let _ = fs::rename(tmp, &state.manifest_path);
    }
}

pub(super) fn prepare_link_cache_status(
    cache_root: &Path,
    target: Option<&str>,
    cmd: &Command,
    obj_paths: &[PathBuf],
    obj_fingerprints: &[String],
    exe_path: &Path,
) -> LinkCacheStatus {
    if std::env::var("PERRY_NO_LINK_CACHE").ok().as_deref() == Some("1") {
        return LinkCacheStatus::linked(None);
    }
    let Some(state) = compute_link_cache_state(
        cache_root,
        target,
        cmd,
        obj_paths,
        obj_fingerprints,
        exe_path,
    ) else {
        return LinkCacheStatus::linked(None);
    };
    let Ok(raw) = fs::read_to_string(&state.manifest_path) else {
        return LinkCacheStatus::linked(Some(state));
    };
    let Ok(manifest) = serde_json::from_str::<LinkCacheManifest>(&raw) else {
        return LinkCacheStatus::linked(Some(state));
    };
    if manifest.version != LINK_CACHE_MANIFEST_VERSION
        || manifest.link_fingerprint != state.link_fingerprint
    {
        return LinkCacheStatus::linked(Some(state));
    }
    let Ok((output_hash, output_size)) = hash_file_with_size(exe_path) else {
        return LinkCacheStatus::linked(Some(state));
    };
    if manifest.output_hash == output_hash && manifest.output_size == output_size {
        LinkCacheStatus::skipped(state)
    } else {
        LinkCacheStatus::linked(Some(state))
    }
}

fn compute_link_cache_state(
    cache_root: &Path,
    target: Option<&str>,
    cmd: &Command,
    obj_paths: &[PathBuf],
    obj_fingerprints: &[String],
    exe_path: &Path,
) -> Option<LinkCacheState> {
    let output_identity = absolute_path_identity(exe_path);
    let mut hasher = Sha256::new();
    feed_hash_field(&mut hasher, "schema", "perry-link-cache-v1");
    feed_hash_field(&mut hasher, "target", target.unwrap_or("native"));
    feed_hash_field(&mut hasher, "output", &output_identity);
    feed_hash_field(&mut hasher, "program", &cmd.get_program().to_string_lossy());

    for arg in cmd.get_args() {
        feed_hash_field(&mut hasher, "arg", &arg.to_string_lossy());
    }
    for (key, value) in cmd.get_envs() {
        feed_hash_field(&mut hasher, "env-key", &key.to_string_lossy());
        feed_hash_field(
            &mut hasher,
            "env-value",
            value
                .map(|v| v.to_string_lossy())
                .as_deref()
                .unwrap_or("<removed>"),
        );
    }

    if let Ok(exe) = std::env::current_exe() {
        feed_file_fingerprint(&mut hasher, "perry", &exe)?;
    }
    if let Some(program_path) = existing_file_from_os_str(cmd.get_program(), exe_path) {
        feed_file_fingerprint(&mut hasher, "program-file", &program_path)?;
    }
    for (obj_path, fingerprint) in obj_paths.iter().zip(obj_fingerprints.iter()) {
        feed_hash_field(
            &mut hasher,
            "object-path",
            &absolute_path_identity(obj_path),
        );
        feed_hash_field(&mut hasher, "object-fingerprint", fingerprint);
    }
    for arg in cmd.get_args() {
        for path in file_inputs_from_arg(arg, exe_path, obj_paths) {
            feed_file_fingerprint(&mut hasher, "input-file", &path)?;
        }
    }

    let link_fingerprint = hex::encode(hasher.finalize());
    let manifest_name = format!(
        "{:016x}.json",
        super::super::object_cache::djb2_hash(output_identity.as_bytes())
    );
    let manifest_path = cache_root
        .join(".perry-cache")
        .join("link")
        .join(target.unwrap_or("native"))
        .join(manifest_name);
    Some(LinkCacheState {
        manifest_path,
        link_fingerprint,
    })
}

fn feed_hash_field(hasher: &mut Sha256, name: &str, value: &str) {
    hasher.update(name.as_bytes());
    hasher.update([0]);
    hasher.update(value.as_bytes());
    hasher.update([0xff]);
}

fn feed_file_fingerprint(hasher: &mut Sha256, role: &str, path: &Path) -> Option<()> {
    let identity = absolute_path_identity(path);
    let (hash, size) = hash_file_with_size(path).ok()?;
    feed_hash_field(hasher, role, &identity);
    feed_hash_field(hasher, "file-size", &size.to_string());
    feed_hash_field(hasher, "file-sha256", &hash);
    Some(())
}

fn hash_file_with_size(path: &Path) -> std::io::Result<(String, u64)> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut size = 0_u64;
    let mut buf = [0_u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        size += n as u64;
        hasher.update(&buf[..n]);
    }
    Ok((hex::encode(hasher.finalize()), size))
}

fn file_inputs_from_arg(
    arg: &std::ffi::OsStr,
    exe_path: &Path,
    obj_paths: &[PathBuf],
) -> Vec<PathBuf> {
    let s = arg.to_string_lossy();
    let mut out = Vec::new();
    if let Some(rest) = s.strip_prefix("/WHOLEARCHIVE:") {
        push_existing_file(&mut out, Path::new(rest), exe_path, obj_paths);
    } else if let Some(rest) = s.strip_prefix("-Wl,-force_load,") {
        push_existing_file(&mut out, Path::new(rest), exe_path, obj_paths);
    } else if s.starts_with("-Wl,-sectcreate,") {
        if let Some(path) = s.rsplit(',').next() {
            push_existing_file(&mut out, Path::new(path), exe_path, obj_paths);
        }
    } else if !s.starts_with('-') && !s.starts_with("/OUT:") {
        push_existing_file(&mut out, Path::new(s.as_ref()), exe_path, obj_paths);
    }
    out
}

fn existing_file_from_os_str(arg: &std::ffi::OsStr, exe_path: &Path) -> Option<PathBuf> {
    let s = arg.to_string_lossy();
    if s.starts_with('-') || s.starts_with("/OUT:") {
        return None;
    }
    let mut out = Vec::new();
    push_existing_file(&mut out, Path::new(s.as_ref()), exe_path, &[]);
    out.into_iter().next()
}

fn push_existing_file(out: &mut Vec<PathBuf>, path: &Path, exe_path: &Path, obj_paths: &[PathBuf]) {
    if same_lexical_path(path, exe_path) {
        return;
    }
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    if same_lexical_path(&candidate, exe_path) {
        return;
    }
    if obj_paths
        .iter()
        .any(|obj_path| same_lexical_path(&candidate, obj_path))
    {
        return;
    }
    if candidate.is_file() {
        out.push(candidate);
    }
}

fn same_lexical_path(a: &Path, b: &Path) -> bool {
    absolute_path_identity(a) == absolute_path_identity(b)
}

fn absolute_path_identity(path: &Path) -> String {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    absolute.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_link_command(input: &Path, output: &Path) -> Command {
        let mut cmd = Command::new("cc");
        cmd.arg(input).arg("-o").arg(output);
        cmd
    }

    #[test]
    fn link_cache_skips_when_command_inputs_and_output_match_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("main.o");
        let output = dir.path().join("app");
        fs::write(&input, b"object-v1").unwrap();
        fs::write(&output, b"binary-v1").unwrap();

        let first = prepare_link_cache_status(
            dir.path(),
            None,
            &fake_link_command(&input, &output),
            &[],
            &[],
            &output,
        );
        assert!(first.stats().linked);
        write_link_cache_manifest(&first, &output);

        let second = prepare_link_cache_status(
            dir.path(),
            None,
            &fake_link_command(&input, &output),
            &[],
            &[],
            &output,
        );
        assert!(second.stats().skipped);
        assert!(!second.stats().linked);
    }

    #[test]
    fn link_cache_relinks_when_input_content_changes() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("main.o");
        let output = dir.path().join("app");
        fs::write(&input, b"object-v1").unwrap();
        fs::write(&output, b"binary-v1").unwrap();

        let first = prepare_link_cache_status(
            dir.path(),
            None,
            &fake_link_command(&input, &output),
            &[],
            &[],
            &output,
        );
        write_link_cache_manifest(&first, &output);

        fs::write(&input, b"object-v2").unwrap();
        let second = prepare_link_cache_status(
            dir.path(),
            None,
            &fake_link_command(&input, &output),
            &[],
            &[],
            &output,
        );
        assert!(second.stats().linked);
        assert!(!second.stats().skipped);
    }

    #[test]
    fn link_cache_relinks_when_output_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("main.o");
        let output = dir.path().join("app");
        fs::write(&input, b"object-v1").unwrap();
        fs::write(&output, b"binary-v1").unwrap();

        let first = prepare_link_cache_status(
            dir.path(),
            None,
            &fake_link_command(&input, &output),
            &[],
            &[],
            &output,
        );
        write_link_cache_manifest(&first, &output);

        fs::remove_file(&output).unwrap();
        let second = prepare_link_cache_status(
            dir.path(),
            None,
            &fake_link_command(&input, &output),
            &[],
            &[],
            &output,
        );
        assert!(second.stats().linked);
        assert!(!second.stats().skipped);
    }
}
