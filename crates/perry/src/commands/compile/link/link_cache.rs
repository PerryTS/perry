use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::super::LinkCacheStats;

const LINK_CACHE_MANIFEST_VERSION: u32 = 1;

const LINK_ENV_VARS: &[&str] = &[
    "PATH",
    "LIB",
    "LIBPATH",
    "LIBRARY_PATH",
    "LD_LIBRARY_PATH",
    "DYLD_LIBRARY_PATH",
    "SDKROOT",
    "PKG_CONFIG_PATH",
    "PKG_CONFIG_LIBDIR",
    "PKG_CONFIG_SYSROOT_DIR",
    "PERRY_LINUX_SYSROOT",
    "PERRY_WINDOWS_SYSROOT",
    "PERRY_IOS_SYSROOT",
    "PERRY_MACOS_SYSROOT",
    "PERRY_TVOS_SYSROOT",
    "PERRY_VISIONOS_SYSROOT",
    "ANDROID_NDK_HOME",
    "OHOS_SDK_HOME",
    "HARMONYOS_SDK_HOME",
];

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
        let input_stats = self
            .state
            .as_ref()
            .map(|state| state.stats)
            .unwrap_or_default();
        LinkCacheStats {
            linked: self.linked,
            skipped: !self.linked,
            object_fingerprints_used: input_stats.object_fingerprints_used,
            object_files_hashed: input_stats.object_files_hashed,
            external_inputs_hashed: input_stats.external_inputs_hashed,
        }
    }
}

#[derive(Debug, Clone)]
struct LinkCacheState {
    manifest_path: PathBuf,
    link_fingerprint: String,
    stats: LinkCacheInputStats,
}

#[derive(Debug, Clone, Copy, Default)]
struct LinkCacheInputStats {
    object_fingerprints_used: usize,
    object_files_hashed: usize,
    external_inputs_hashed: usize,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct LinkCacheManifest {
    version: u32,
    link_fingerprint: String,
    output_hash: String,
    output_size: u64,
}

#[derive(Debug, Clone)]
struct SearchDir {
    path: PathBuf,
    system: bool,
}

struct LinkFingerprintContext<'a> {
    cwd: PathBuf,
    exe_path: &'a Path,
    obj_paths: &'a [PathBuf],
    stats: LinkCacheInputStats,
    lib_dirs: Vec<SearchDir>,
    framework_dirs: Vec<SearchDir>,
    msvc_lib_dirs: Vec<SearchDir>,
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
    cache_dir: &Path,
    target: Option<&str>,
    cmd: &Command,
    obj_paths: &[PathBuf],
    obj_fingerprints: &[Option<String>],
    exe_path: &Path,
) -> LinkCacheStatus {
    if std::env::var("PERRY_NO_LINK_CACHE").ok().as_deref() == Some("1") {
        return LinkCacheStatus::linked(None);
    }
    let Some(state) = compute_link_cache_state(
        cache_dir,
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
    cache_dir: &Path,
    target: Option<&str>,
    cmd: &Command,
    obj_paths: &[PathBuf],
    obj_fingerprints: &[Option<String>],
    exe_path: &Path,
) -> Option<LinkCacheState> {
    let cwd = command_cwd(cmd);
    let output_identity = absolute_output_identity_from(exe_path, &cwd);
    let mut hasher = Sha256::new();
    feed_hash_field(&mut hasher, "schema", "perry-link-cache-v2");
    feed_hash_field(&mut hasher, "target", target.unwrap_or("native"));
    feed_hash_field(&mut hasher, "output", &output_identity);

    feed_effective_env(&mut hasher, cmd);
    feed_program_fingerprint(&mut hasher, cmd, &cwd)?;
    if let Ok(exe) = std::env::current_exe() {
        feed_file_fingerprint_from(&mut hasher, "perry", &exe, &cwd)?;
    }

    let mut stats = LinkCacheInputStats::default();
    for (idx, obj_path) in obj_paths.iter().enumerate() {
        feed_hash_field(&mut hasher, "object-index", &idx.to_string());
        if let Some(fingerprint) = trusted_object_fingerprint(obj_path, obj_fingerprints.get(idx)) {
            feed_hash_field(
                &mut hasher,
                "object-file",
                &absolute_path_identity_from(obj_path, &cwd),
            );
            feed_hash_field(&mut hasher, "object-fingerprint", fingerprint);
            stats.object_fingerprints_used += 1;
        } else {
            feed_file_fingerprint_from(&mut hasher, "object-file", obj_path, &cwd)?;
            stats.object_files_hashed += 1;
        }
    }

    let mut ctx = LinkFingerprintContext {
        cwd: cwd.clone(),
        exe_path,
        obj_paths,
        stats,
        lib_dirs: library_path_search_dirs(cmd, &cwd),
        framework_dirs: default_framework_search_dirs(cmd, &cwd),
        msvc_lib_dirs: msvc_lib_search_dirs(cmd, &cwd),
    };
    feed_command_args(&mut hasher, cmd, &mut ctx)?;
    let stats = ctx.stats;

    let link_fingerprint = hex::encode(hasher.finalize());
    let manifest_name = format!(
        "{:016x}.json",
        super::super::object_cache::djb2_hash(output_identity.as_bytes())
    );
    let manifest_path = cache_dir
        .join("link")
        .join(target.unwrap_or("native"))
        .join(manifest_name);
    Some(LinkCacheState {
        manifest_path,
        link_fingerprint,
        stats,
    })
}

fn trusted_object_fingerprint<'a>(
    obj_path: &Path,
    fingerprint: Option<&'a Option<String>>,
) -> Option<&'a str> {
    let fingerprint = fingerprint?.as_deref()?;
    let metadata = fs::metadata(obj_path).ok()?;
    if fingerprint.is_empty() || !metadata.is_file() || metadata.len() == 0 {
        return None;
    }
    Some(fingerprint)
}

fn feed_effective_env(hasher: &mut Sha256, cmd: &Command) {
    for name in LINK_ENV_VARS {
        feed_hash_field(hasher, "env-key", name);
        match effective_env_value(cmd, name) {
            Some(value) => feed_hash_field(hasher, "env-value", &value.to_string_lossy()),
            None => feed_hash_field(hasher, "env-value", "<unset>"),
        }
    }
}

fn feed_program_fingerprint(hasher: &mut Sha256, cmd: &Command, cwd: &Path) -> Option<()> {
    feed_hash_field(hasher, "program", &cmd.get_program().to_string_lossy());
    if let Some(program_path) = resolve_program(cmd.get_program(), cmd, cwd) {
        feed_file_fingerprint_from(hasher, "program-file", &program_path, cwd)?;
    }
    Some(())
}

fn feed_command_args(
    hasher: &mut Sha256,
    cmd: &Command,
    ctx: &mut LinkFingerprintContext<'_>,
) -> Option<()> {
    let args: Vec<OsString> = cmd.get_args().map(OsStr::to_os_string).collect();
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].to_string_lossy();
        feed_hash_field(hasher, "arg-index", &i.to_string());

        if arg == "-o" {
            feed_hash_field(hasher, "arg", "-o");
            if let Some(next) = args.get(i + 1) {
                feed_output_arg(hasher, next, ctx);
                i += 2;
                continue;
            }
        } else if let Some(rest) = arg.strip_prefix("/OUT:") {
            feed_hash_field(hasher, "arg", "/OUT");
            feed_output_arg(hasher, OsStr::new(rest), ctx);
            i += 1;
            continue;
        } else if arg == "-L" {
            feed_hash_field(hasher, "arg", "-L");
            if let Some(next) = args.get(i + 1) {
                add_and_feed_search_dir(hasher, &mut ctx.lib_dirs, next.as_os_str(), &ctx.cwd);
                i += 2;
                continue;
            }
        } else if let Some(rest) = arg.strip_prefix("-L") {
            feed_hash_field(hasher, "arg", "-L");
            add_and_feed_search_dir(hasher, &mut ctx.lib_dirs, OsStr::new(rest), &ctx.cwd);
            i += 1;
            continue;
        } else if arg == "-F" {
            feed_hash_field(hasher, "arg", "-F");
            if let Some(next) = args.get(i + 1) {
                add_and_feed_search_dir(
                    hasher,
                    &mut ctx.framework_dirs,
                    next.as_os_str(),
                    &ctx.cwd,
                );
                i += 2;
                continue;
            }
        } else if let Some(rest) = arg.strip_prefix("-F") {
            feed_hash_field(hasher, "arg", "-F");
            add_and_feed_search_dir(hasher, &mut ctx.framework_dirs, OsStr::new(rest), &ctx.cwd);
            i += 1;
            continue;
        } else if let Some(rest) = arg.strip_prefix("/LIBPATH:") {
            feed_hash_field(hasher, "arg", "/LIBPATH");
            add_and_feed_search_dir(hasher, &mut ctx.msvc_lib_dirs, OsStr::new(rest), &ctx.cwd);
            i += 1;
            continue;
        } else if arg == "-l" {
            feed_hash_field(hasher, "arg", "-l");
            if let Some(next) = args.get(i + 1) {
                feed_unix_library(hasher, &next.to_string_lossy(), ctx)?;
                i += 2;
                continue;
            }
        } else if let Some(rest) = arg.strip_prefix("-l") {
            feed_hash_field(hasher, "arg", "-l");
            feed_unix_library(hasher, rest, ctx)?;
            i += 1;
            continue;
        } else if arg == "-framework" {
            feed_hash_field(hasher, "arg", "-framework");
            if let Some(next) = args.get(i + 1) {
                feed_framework(hasher, &next.to_string_lossy(), ctx)?;
                i += 2;
                continue;
            }
        } else if let Some(rest) = arg.strip_prefix("/WHOLEARCHIVE:") {
            feed_hash_field(hasher, "arg", "/WHOLEARCHIVE");
            feed_explicit_file_arg(hasher, "wholearchive", OsStr::new(rest), ctx)?;
            i += 1;
            continue;
        } else if arg.starts_with("-Wl,") {
            feed_wl_arg(hasher, &arg, ctx)?;
            i += 1;
            continue;
        } else if arg == "-sectcreate" {
            feed_hash_field(hasher, "arg", "-sectcreate");
            if args.len() > i + 3 {
                feed_hash_field(hasher, "sectcreate-segment", &args[i + 1].to_string_lossy());
                feed_hash_field(hasher, "sectcreate-section", &args[i + 2].to_string_lossy());
                feed_file_content_arg(hasher, "sectcreate-file", &args[i + 3], ctx)?;
                i += 4;
                continue;
            }
        } else if looks_like_msvc_lib(&arg) {
            feed_hash_field(hasher, "arg", "msvc-lib");
            feed_msvc_library(hasher, &arg, ctx)?;
            i += 1;
            continue;
        } else if looks_like_file_arg(&arg) {
            feed_explicit_file_arg(hasher, "input-file", args[i].as_os_str(), ctx)?;
            i += 1;
            continue;
        }

        feed_hash_field(hasher, "arg", &arg);
        i += 1;
    }
    Some(())
}

fn feed_wl_arg(hasher: &mut Sha256, arg: &str, ctx: &mut LinkFingerprintContext<'_>) -> Option<()> {
    let payload = arg.strip_prefix("-Wl,").unwrap_or(arg);
    let parts: Vec<&str> = payload.split(',').collect();
    feed_hash_field(hasher, "arg", "-Wl");

    if parts.len() >= 2 && parts[0] == "-force_load" {
        feed_hash_field(hasher, "wl-op", "-force_load");
        feed_explicit_file_arg(hasher, "force-load", OsStr::new(parts[1]), ctx)?;
    } else if parts.len() >= 4 && parts[0] == "-sectcreate" {
        feed_hash_field(hasher, "wl-op", "-sectcreate");
        feed_hash_field(hasher, "sectcreate-segment", parts[1]);
        feed_hash_field(hasher, "sectcreate-section", parts[2]);
        feed_file_content_arg(hasher, "sectcreate-file", OsStr::new(parts[3]), ctx)?;
    } else if parts.len() >= 2 && parts[0] == "-framework" {
        feed_hash_field(hasher, "wl-op", "-framework");
        feed_framework(hasher, parts[1], ctx)?;
    } else {
        feed_hash_field(hasher, "wl", payload);
    }
    Some(())
}

fn feed_output_arg(hasher: &mut Sha256, output: &OsStr, ctx: &LinkFingerprintContext<'_>) {
    let output_identity = absolute_output_identity_from(Path::new(output), &ctx.cwd);
    feed_hash_field(hasher, "output-arg", &output_identity);
}

fn feed_explicit_file_arg(
    hasher: &mut Sha256,
    role: &str,
    path: &OsStr,
    ctx: &mut LinkFingerprintContext<'_>,
) -> Option<()> {
    let candidate = resolve_relative_path(Path::new(path), &ctx.cwd);
    if same_path_from(&candidate, ctx.exe_path, &ctx.cwd) {
        feed_hash_field(hasher, role, "<output>");
        return Some(());
    }
    if ctx
        .obj_paths
        .iter()
        .any(|obj_path| same_path_from(&candidate, obj_path, &ctx.cwd))
    {
        feed_hash_field(
            hasher,
            role,
            &absolute_path_identity_from(&candidate, &ctx.cwd),
        );
        return Some(());
    }
    if candidate.is_file() {
        feed_external_file_fingerprint_from(hasher, role, &candidate, ctx)
    } else {
        feed_hash_field(hasher, role, &path.to_string_lossy());
        Some(())
    }
}

fn feed_file_content_arg(
    hasher: &mut Sha256,
    role: &str,
    path: &OsStr,
    ctx: &mut LinkFingerprintContext<'_>,
) -> Option<()> {
    let candidate = resolve_relative_path(Path::new(path), &ctx.cwd);
    let (hash, size) = hash_file_with_size(&candidate).ok()?;
    ctx.stats.external_inputs_hashed += 1;
    feed_hash_field(hasher, role, "<content-only>");
    feed_hash_field(hasher, "file-size", &size.to_string());
    feed_hash_field(hasher, "file-sha256", &hash);
    Some(())
}

fn feed_unix_library(
    hasher: &mut Sha256,
    name: &str,
    ctx: &mut LinkFingerprintContext<'_>,
) -> Option<()> {
    feed_hash_field(hasher, "library", name);
    if let Some(path) = resolve_unix_library(name, &ctx.lib_dirs) {
        return feed_external_file_fingerprint_from(hasher, "library-file", &path, ctx);
    }
    if has_non_system_dir(&ctx.lib_dirs) {
        return None;
    }
    feed_hash_field(hasher, "library-file", "<system-unresolved>");
    Some(())
}

fn feed_msvc_library(
    hasher: &mut Sha256,
    name: &str,
    ctx: &mut LinkFingerprintContext<'_>,
) -> Option<()> {
    feed_hash_field(hasher, "msvc-library", name);
    let direct = resolve_relative_path(Path::new(name), &ctx.cwd);
    if direct.is_file() {
        return feed_external_file_fingerprint_from(hasher, "msvc-library-file", &direct, ctx);
    }
    if let Some(path) = resolve_msvc_library(name, &ctx.msvc_lib_dirs) {
        return feed_external_file_fingerprint_from(hasher, "msvc-library-file", &path, ctx);
    }
    if has_non_system_dir(&ctx.msvc_lib_dirs) {
        return None;
    }
    feed_hash_field(hasher, "msvc-library-file", "<system-unresolved>");
    Some(())
}

fn feed_framework(
    hasher: &mut Sha256,
    name: &str,
    ctx: &mut LinkFingerprintContext<'_>,
) -> Option<()> {
    feed_hash_field(hasher, "framework", name);
    if let Some(path) = resolve_framework(name, &ctx.framework_dirs) {
        return feed_external_file_fingerprint_from(hasher, "framework-file", &path, ctx);
    }
    if has_non_system_dir(&ctx.framework_dirs) {
        return None;
    }
    feed_hash_field(hasher, "framework-file", "<system-unresolved>");
    Some(())
}

fn add_and_feed_search_dir(
    hasher: &mut Sha256,
    dirs: &mut Vec<SearchDir>,
    path: &OsStr,
    cwd: &Path,
) {
    let resolved = resolve_relative_path(Path::new(path), cwd);
    let system = is_system_search_dir(&resolved);
    feed_hash_field(
        hasher,
        "search-dir",
        &absolute_path_identity_from(&resolved, cwd),
    );
    feed_hash_field(hasher, "search-dir-system", if system { "1" } else { "0" });
    dirs.push(SearchDir {
        path: resolved,
        system,
    });
}

fn library_path_search_dirs(cmd: &Command, cwd: &Path) -> Vec<SearchDir> {
    split_env_paths(cmd, "LIBRARY_PATH", cwd, true)
}

fn default_framework_search_dirs(cmd: &Command, cwd: &Path) -> Vec<SearchDir> {
    let mut dirs = Vec::new();
    if let Some(sdkroot) = effective_env_value(cmd, "SDKROOT") {
        let sdk = PathBuf::from(sdkroot);
        dirs.push(SearchDir {
            path: sdk.join("System/Library/Frameworks"),
            system: true,
        });
    }
    dirs.push(SearchDir {
        path: PathBuf::from("/System/Library/Frameworks"),
        system: true,
    });
    dirs.push(SearchDir {
        path: resolve_relative_path(Path::new("System/Library/Frameworks"), cwd),
        system: true,
    });
    dirs
}

fn msvc_lib_search_dirs(cmd: &Command, cwd: &Path) -> Vec<SearchDir> {
    let mut dirs = split_env_paths(cmd, "LIB", cwd, true);
    dirs.extend(split_env_paths(cmd, "LIBPATH", cwd, true));
    dirs
}

fn split_env_paths(cmd: &Command, name: &str, cwd: &Path, system: bool) -> Vec<SearchDir> {
    effective_env_value(cmd, name)
        .map(|value| {
            std::env::split_paths(&value)
                .map(|path| {
                    let resolved = resolve_relative_path(&path, cwd);
                    SearchDir {
                        system: system || is_system_search_dir(&resolved),
                        path: resolved,
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

fn resolve_unix_library(name: &str, dirs: &[SearchDir]) -> Option<PathBuf> {
    let names = unix_library_file_names(name);
    for dir in dirs {
        for candidate_name in &names {
            let candidate = dir.path.join(candidate_name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn resolve_msvc_library(name: &str, dirs: &[SearchDir]) -> Option<PathBuf> {
    let names = msvc_library_file_names(name);
    for dir in dirs {
        for candidate_name in &names {
            let candidate = dir.path.join(candidate_name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn resolve_framework(name: &str, dirs: &[SearchDir]) -> Option<PathBuf> {
    for dir in dirs {
        let fw_dir = dir.path.join(format!("{name}.framework"));
        for candidate in [
            fw_dir.join(name),
            fw_dir.join(format!("{name}.tbd")),
            fw_dir.join("Versions/Current").join(name),
            fw_dir.join("Versions/A").join(name),
        ] {
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn unix_library_file_names(name: &str) -> Vec<String> {
    if let Some(exact) = name.strip_prefix(':') {
        return vec![exact.to_string()];
    }
    if name.ends_with(".a")
        || name.ends_with(".so")
        || name.ends_with(".dylib")
        || name.ends_with(".tbd")
    {
        return vec![name.to_string()];
    }
    vec![
        format!("lib{name}.so"),
        format!("lib{name}.a"),
        format!("lib{name}.dylib"),
        format!("lib{name}.tbd"),
    ]
}

fn msvc_library_file_names(name: &str) -> Vec<String> {
    if name.ends_with(".lib") {
        vec![name.to_string()]
    } else {
        vec![format!("{name}.lib"), format!("lib{name}.lib")]
    }
}

fn has_non_system_dir(dirs: &[SearchDir]) -> bool {
    dirs.iter().any(|dir| !dir.system)
}

fn looks_like_msvc_lib(arg: &str) -> bool {
    arg.ends_with(".lib") && !arg.contains('/') && !arg.contains('\\')
}

fn looks_like_file_arg(arg: &str) -> bool {
    if arg.starts_with('-') || arg.starts_with("/OUT:") || arg.starts_with("/LIBPATH:") {
        return false;
    }
    let path = Path::new(arg);
    path.is_absolute() || arg.contains('/') || arg.contains('\\') || path.extension().is_some()
}

fn is_system_search_dir(path: &Path) -> bool {
    let s = path
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();
    s.starts_with("/usr/")
        || s == "/usr"
        || s.starts_with("/lib/")
        || s == "/lib"
        || s.starts_with("/system/")
        || s.starts_with("/applications/xcode")
        || s.starts_with("/library/developer")
        || s.starts_with("/opt/homebrew")
        || s.starts_with("/usr/local")
        || s.contains("/windows kits/")
        || s.contains("/microsoft visual studio/")
}

fn feed_hash_field(hasher: &mut Sha256, name: &str, value: &str) {
    hasher.update(name.as_bytes());
    hasher.update([0]);
    hasher.update(value.as_bytes());
    hasher.update([0xff]);
}

fn feed_external_file_fingerprint_from(
    hasher: &mut Sha256,
    role: &str,
    path: &Path,
    ctx: &mut LinkFingerprintContext<'_>,
) -> Option<()> {
    feed_file_fingerprint_from(hasher, role, path, &ctx.cwd)?;
    ctx.stats.external_inputs_hashed += 1;
    Some(())
}

fn feed_file_fingerprint_from(
    hasher: &mut Sha256,
    role: &str,
    path: &Path,
    cwd: &Path,
) -> Option<()> {
    let identity = absolute_path_identity_from(path, cwd);
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

fn resolve_program(program: &OsStr, cmd: &Command, cwd: &Path) -> Option<PathBuf> {
    let program_path = Path::new(program);
    if program_path.is_absolute() || has_path_separator(program) {
        let candidate = resolve_relative_path(program_path, cwd);
        return candidate.is_file().then_some(candidate);
    }

    let path_value = effective_env_value(cmd, "PATH")?;
    for dir in std::env::split_paths(&path_value) {
        for candidate in program_candidates(program) {
            let path = dir.join(candidate);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

fn program_candidates(program: &OsStr) -> Vec<OsString> {
    #[cfg(windows)]
    {
        let mut candidates = vec![program.to_os_string()];
        if Path::new(program).extension().is_none() {
            let pathext = std::env::var_os("PATHEXT")
                .unwrap_or_else(|| OsString::from(".COM;.EXE;.BAT;.CMD"));
            for ext in pathext.to_string_lossy().split(';') {
                let mut candidate = program.to_os_string();
                candidate.push(ext.to_ascii_lowercase());
                candidates.push(candidate);
            }
        }
        candidates
    }
    #[cfg(not(windows))]
    {
        vec![program.to_os_string()]
    }
}

fn effective_env_value(cmd: &Command, name: &str) -> Option<OsString> {
    match explicit_env_value(cmd, name) {
        Some(Some(value)) => Some(value),
        Some(None) => None,
        None => std::env::var_os(name),
    }
}

fn explicit_env_value(cmd: &Command, name: &str) -> Option<Option<OsString>> {
    for (key, value) in cmd.get_envs() {
        if key == OsStr::new(name) {
            return Some(value.map(OsStr::to_os_string));
        }
    }
    None
}

fn command_cwd(cmd: &Command) -> PathBuf {
    cmd.get_current_dir()
        .map(Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn has_path_separator(path: &OsStr) -> bool {
    let s = path.to_string_lossy();
    s.contains('/') || s.contains('\\')
}

fn same_path_from(a: &Path, b: &Path, cwd: &Path) -> bool {
    absolute_path_identity_from(a, cwd) == absolute_path_identity_from(b, cwd)
}

fn absolute_path_identity_from(path: &Path, cwd: &Path) -> String {
    let absolute = resolve_relative_path(path, cwd);
    absolute
        .canonicalize()
        .unwrap_or(absolute)
        .to_string_lossy()
        .into_owned()
}

fn absolute_output_identity_from(path: &Path, cwd: &Path) -> String {
    resolve_relative_path(path, cwd)
        .to_string_lossy()
        .into_owned()
}

fn resolve_relative_path(path: &Path, cwd: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_link_command(project: &Path, input: &Path, output: &Path) -> Command {
        let bin = project.join("bin");
        fs::create_dir_all(&bin).unwrap();
        fs::write(bin.join("cc"), b"fake-linker-v1").unwrap();
        let mut cmd = Command::new("cc");
        cmd.current_dir(project)
            .env("PATH", &bin)
            .arg(input)
            .arg("-o")
            .arg(output);
        cmd
    }

    fn write_manifest_for(project: &Path, cmd: &Command, output: &Path, obj_paths: &[PathBuf]) {
        write_manifest_for_with_fingerprints(project, cmd, output, obj_paths, &[]);
    }

    fn write_manifest_for_with_fingerprints(
        project: &Path,
        cmd: &Command,
        output: &Path,
        obj_paths: &[PathBuf],
        obj_fingerprints: &[Option<String>],
    ) {
        let first =
            prepare_link_cache_status(project, None, cmd, obj_paths, obj_fingerprints, output);
        assert!(first.stats().linked);
        write_link_cache_manifest(&first, output);
    }

    fn assert_skips(
        project: &Path,
        cmd: &Command,
        output: &Path,
        obj_paths: &[PathBuf],
    ) -> LinkCacheStats {
        let status = prepare_link_cache_status(project, None, cmd, obj_paths, &[], output);
        assert!(status.stats().skipped);
        assert!(!status.stats().linked);
        status.stats()
    }

    fn assert_links(
        project: &Path,
        cmd: &Command,
        output: &Path,
        obj_paths: &[PathBuf],
    ) -> LinkCacheStats {
        let status = prepare_link_cache_status(project, None, cmd, obj_paths, &[], output);
        assert!(status.stats().linked);
        assert!(!status.stats().skipped);
        status.stats()
    }

    fn assert_skips_with_fingerprints(
        project: &Path,
        cmd: &Command,
        output: &Path,
        obj_paths: &[PathBuf],
        obj_fingerprints: &[Option<String>],
    ) -> LinkCacheStats {
        let status =
            prepare_link_cache_status(project, None, cmd, obj_paths, obj_fingerprints, output);
        assert!(status.stats().skipped);
        assert!(!status.stats().linked);
        status.stats()
    }

    fn assert_links_with_fingerprints(
        project: &Path,
        cmd: &Command,
        output: &Path,
        obj_paths: &[PathBuf],
        obj_fingerprints: &[Option<String>],
    ) -> LinkCacheStats {
        let status =
            prepare_link_cache_status(project, None, cmd, obj_paths, obj_fingerprints, output);
        assert!(status.stats().linked);
        assert!(!status.stats().skipped);
        status.stats()
    }

    #[test]
    fn link_cache_skips_when_command_inputs_and_output_match_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path();
        let input = project.join("main.o");
        let output = project.join("app");
        fs::write(&input, b"object-v1").unwrap();
        fs::write(&output, b"binary-v1").unwrap();
        let cmd = fake_link_command(project, &input, &output);

        write_manifest_for(project, &cmd, &output, &[input.clone()]);
        assert_skips(project, &cmd, &output, &[input]);
    }

    #[test]
    fn link_cache_relinks_when_input_content_changes() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path();
        let input = project.join("main.o");
        let output = project.join("app");
        fs::write(&input, b"object-v1").unwrap();
        fs::write(&output, b"binary-v1").unwrap();
        let cmd = fake_link_command(project, &input, &output);

        write_manifest_for(project, &cmd, &output, &[input.clone()]);

        fs::write(&input, b"object-v2").unwrap();
        assert_links(project, &cmd, &output, &[input]);
    }

    #[test]
    fn link_cache_uses_stable_object_fingerprints_without_hashing_object_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path();
        let input = project.join("main.o");
        let output = project.join("app");
        fs::write(&input, b"object-v1").unwrap();
        fs::write(&output, b"binary-v1").unwrap();
        let cmd = fake_link_command(project, &input, &output);
        let objects = vec![input.clone()];
        let fingerprints = vec![Some("perry-object-cache:fingerprint-v1".to_string())];

        write_manifest_for_with_fingerprints(project, &cmd, &output, &objects, &fingerprints);

        fs::write(&input, b"object-v2-not-read-by-link-cache").unwrap();
        let stats = assert_skips_with_fingerprints(project, &cmd, &output, &objects, &fingerprints);
        assert_eq!(stats.object_fingerprints_used, 1);
        assert_eq!(stats.object_files_hashed, 0);
    }

    #[test]
    fn link_cache_relinks_when_object_fingerprint_changes() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path();
        let input = project.join("main.o");
        let output = project.join("app");
        fs::write(&input, b"object-v1").unwrap();
        fs::write(&output, b"binary-v1").unwrap();
        let cmd = fake_link_command(project, &input, &output);
        let objects = vec![input.clone()];
        let old_fingerprints = vec![Some("perry-object-cache:fingerprint-v1".to_string())];
        let new_fingerprints = vec![Some("perry-object-cache:fingerprint-v2".to_string())];

        write_manifest_for_with_fingerprints(project, &cmd, &output, &objects, &old_fingerprints);

        let stats =
            assert_links_with_fingerprints(project, &cmd, &output, &objects, &new_fingerprints);
        assert_eq!(stats.object_fingerprints_used, 1);
        assert_eq!(stats.object_files_hashed, 0);
    }

    #[test]
    fn link_cache_relinks_when_fingerprinted_object_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path();
        let input = project.join("main.o");
        let output = project.join("app");
        fs::write(&input, b"object-v1").unwrap();
        fs::write(&output, b"binary-v1").unwrap();
        let cmd = fake_link_command(project, &input, &output);
        let objects = vec![input.clone()];
        let fingerprints = vec![Some("perry-object-cache:fingerprint-v1".to_string())];

        write_manifest_for_with_fingerprints(project, &cmd, &output, &objects, &fingerprints);

        fs::remove_file(&input).unwrap();
        assert_links_with_fingerprints(project, &cmd, &output, &objects, &fingerprints);
    }

    #[test]
    fn link_cache_hashes_truncated_fingerprinted_object() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path();
        let input = project.join("main.o");
        let output = project.join("app");
        fs::write(&input, b"object-v1").unwrap();
        fs::write(&output, b"binary-v1").unwrap();
        let cmd = fake_link_command(project, &input, &output);
        let objects = vec![input.clone()];
        let fingerprints = vec![Some("perry-object-cache:fingerprint-v1".to_string())];

        write_manifest_for_with_fingerprints(project, &cmd, &output, &objects, &fingerprints);

        fs::write(&input, b"").unwrap();
        let stats = assert_links_with_fingerprints(project, &cmd, &output, &objects, &fingerprints);
        assert_eq!(stats.object_fingerprints_used, 0);
        assert_eq!(stats.object_files_hashed, 1);
    }

    #[test]
    fn link_cache_relinks_when_output_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path();
        let input = project.join("main.o");
        let output = project.join("app");
        fs::write(&input, b"object-v1").unwrap();
        fs::write(&output, b"binary-v1").unwrap();
        let cmd = fake_link_command(project, &input, &output);

        write_manifest_for(project, &cmd, &output, &[input.clone()]);

        fs::remove_file(&output).unwrap();
        assert_links(project, &cmd, &output, &[input]);
    }

    #[cfg(unix)]
    #[test]
    fn link_cache_output_identity_does_not_change_after_output_is_created() {
        let dir = tempfile::tempdir().unwrap();
        let real_project = dir.path().join("real");
        let alias_project = dir.path().join("alias");
        fs::create_dir_all(&real_project).unwrap();
        std::os::unix::fs::symlink(&real_project, &alias_project).unwrap();

        let input = alias_project.join("main.o");
        let output = alias_project.join("app");
        fs::write(&input, b"object-v1").unwrap();
        let cmd = fake_link_command(&alias_project, &input, &output);

        let first =
            prepare_link_cache_status(&alias_project, None, &cmd, &[input.clone()], &[], &output);
        assert!(first.stats().linked);
        fs::write(&output, b"binary-v1").unwrap();
        write_link_cache_manifest(&first, &output);

        assert_skips(&alias_project, &cmd, &output, &[input]);
    }

    #[test]
    fn link_cache_relinks_when_l_search_library_changes() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path();
        let input = project.join("main.o");
        let output = project.join("app");
        let lib_dir = project.join("vendor/lib");
        fs::create_dir_all(&lib_dir).unwrap();
        fs::write(&input, b"object-v1").unwrap();
        fs::write(&output, b"binary-v1").unwrap();
        fs::write(lib_dir.join("libfoo.a"), b"foo-v1").unwrap();
        let mut cmd = fake_link_command(project, &input, &output);
        cmd.arg("-Lvendor/lib").arg("-lfoo");

        write_manifest_for(project, &cmd, &output, &[input.clone()]);
        assert_skips(project, &cmd, &output, &[input.clone()]);

        fs::write(lib_dir.join("libfoo.a"), b"foo-v2").unwrap();
        assert_links(project, &cmd, &output, &[input]);
    }

    #[test]
    fn link_cache_relinks_when_msvc_libpath_library_changes() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path();
        let bin = project.join("bin");
        let lib_dir = project.join("msvc-lib");
        let input = project.join("main.obj");
        let output = project.join("app.exe");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(&lib_dir).unwrap();
        fs::write(bin.join("link.exe"), b"fake-linker-v1").unwrap();
        fs::write(&input, b"object-v1").unwrap();
        fs::write(&output, b"binary-v1").unwrap();
        fs::write(lib_dir.join("foo.lib"), b"foo-v1").unwrap();
        let mut cmd = Command::new("link.exe");
        cmd.current_dir(project)
            .env("PATH", &bin)
            .arg(&input)
            .arg(format!("/LIBPATH:{}", lib_dir.display()))
            .arg("foo.lib")
            .arg(format!("/OUT:{}", output.display()));

        write_manifest_for(project, &cmd, &output, &[input.clone()]);
        assert_skips(project, &cmd, &output, &[input.clone()]);

        fs::write(lib_dir.join("foo.lib"), b"foo-v2").unwrap();
        assert_links(project, &cmd, &output, &[input]);
    }

    #[test]
    fn link_cache_relinks_when_framework_binary_changes() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path();
        let input = project.join("main.o");
        let output = project.join("app");
        let fw_binary = project.join("Vendor/Foo.framework/Foo");
        fs::create_dir_all(fw_binary.parent().unwrap()).unwrap();
        fs::write(&input, b"object-v1").unwrap();
        fs::write(&output, b"binary-v1").unwrap();
        fs::write(&fw_binary, b"foo-v1").unwrap();
        let mut cmd = fake_link_command(project, &input, &output);
        cmd.arg("-FVendor").arg("-framework").arg("Foo");

        write_manifest_for(project, &cmd, &output, &[input.clone()]);
        assert_skips(project, &cmd, &output, &[input.clone()]);

        fs::write(&fw_binary, b"foo-v2").unwrap();
        assert_links(project, &cmd, &output, &[input]);
    }

    #[test]
    fn link_cache_relinks_when_resolved_program_changes() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path();
        let input = project.join("main.o");
        let output = project.join("app");
        fs::write(&input, b"object-v1").unwrap();
        fs::write(&output, b"binary-v1").unwrap();
        let cmd = fake_link_command(project, &input, &output);

        write_manifest_for(project, &cmd, &output, &[input.clone()]);
        assert_skips(project, &cmd, &output, &[input.clone()]);

        fs::write(project.join("bin/cc"), b"fake-linker-v2").unwrap();
        assert_links(project, &cmd, &output, &[input]);
    }

    #[test]
    fn link_cache_hashes_extra_object_paths_without_precomputed_fingerprints() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path();
        let input = project.join("main.o");
        let extra = project.join("extra.o");
        let output = project.join("app");
        fs::write(&input, b"object-v1").unwrap();
        fs::write(&extra, b"extra-v1").unwrap();
        fs::write(&output, b"binary-v1").unwrap();
        let mut cmd = fake_link_command(project, &input, &output);
        cmd.arg(&extra);
        let objects = vec![input.clone(), extra.clone()];

        let fingerprints = vec![Some("perry-object-cache:main-v1".to_string()), None];

        write_manifest_for_with_fingerprints(project, &cmd, &output, &objects, &fingerprints);
        let stats = assert_skips_with_fingerprints(project, &cmd, &output, &objects, &fingerprints);
        assert_eq!(stats.object_fingerprints_used, 1);
        assert_eq!(stats.object_files_hashed, 1);

        fs::write(&extra, b"extra-v2").unwrap();
        let stats = assert_links_with_fingerprints(project, &cmd, &output, &objects, &fingerprints);
        assert_eq!(stats.object_fingerprints_used, 1);
        assert_eq!(stats.object_files_hashed, 1);
    }

    #[test]
    fn link_cache_hashes_sectcreate_file_content_not_temp_path() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path();
        let input = project.join("main.o");
        let output = project.join("app");
        let first_plist = project.join("tmp-a/info.plist");
        let second_plist = project.join("tmp-b/info.plist");
        fs::create_dir_all(first_plist.parent().unwrap()).unwrap();
        fs::create_dir_all(second_plist.parent().unwrap()).unwrap();
        fs::write(&input, b"object-v1").unwrap();
        fs::write(&output, b"binary-v1").unwrap();
        fs::write(&first_plist, b"<plist/>").unwrap();
        fs::write(&second_plist, b"<plist/>").unwrap();

        let mut first = fake_link_command(project, &input, &output);
        first.arg("-Wl,-sectcreate,__TEXT,__info_plist,tmp-a/info.plist");
        let mut second = fake_link_command(project, &input, &output);
        second.arg("-Wl,-sectcreate,__TEXT,__info_plist,tmp-b/info.plist");

        write_manifest_for(project, &first, &output, &[input.clone()]);
        assert_skips(project, &second, &output, &[input.clone()]);

        fs::write(&second_plist, b"<plist><changed/></plist>").unwrap();
        assert_links(project, &second, &output, &[input]);
    }
}
