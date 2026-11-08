//! Desktop platform glue (macOS / Windows / Linux).
//!
//! Module of `perry-updater` — atomic replace of the running executable,
//! well-known per-OS path resolution, and detached relaunch via `setsid`
//! (Unix) or `DETACHED_PROCESS` (Windows). cfg(target_os) gates the per-OS
//! arms; mobile builds reach the unix branch and produce non-fatal results
//! (callers should gate updater code with `os.platform()` since update
//! delivery on iOS/Android goes through the OS store anyway).

use perry_runtime::{js_string_from_bytes, StringHeader};

/// Extract a Rust String from a raw `*const StringHeader` pointer (passed as
/// `i64` over the FFI). The codegen extracts the heap pointer via
/// `js_get_string_pointer_unified` for `UiArgKind::Str` args.
unsafe fn extract_str(ptr_val: i64) -> Option<String> {
    if ptr_val == 0 || (ptr_val as usize) < 0x1000 {
        return None;
    }
    let ptr = ptr_val as *const StringHeader;
    let len = (*ptr).byte_len as usize;
    let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data, len);
    std::str::from_utf8(bytes).ok().map(|s| s.to_string())
}

fn alloc_string(s: &str) -> *mut StringHeader {
    js_string_from_bytes(s.as_ptr(), s.len() as u32)
}

// ============================================================================
// Path resolution
// ============================================================================

/// Resolve the path to the running executable, accounting for platform
/// quirks. macOS: walks up to the surrounding `.app` bundle if applicable.
/// Linux: honors `$APPIMAGE` when set (the AppImage runtime points
/// `current_exe()` inside the squashfs mount, which is read-only — the real
/// target to replace is the AppImage file itself).
#[no_mangle]
pub extern "C" fn perry_updater_get_exe_path() -> *mut StringHeader {
    // AppImage detection (Linux): $APPIMAGE points at the actual file on disk.
    if let Ok(app_image) = std::env::var("APPIMAGE") {
        if !app_image.is_empty() {
            return alloc_string(&app_image);
        }
    }

    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return alloc_string(""),
    };

    let canonical = exe.canonicalize().unwrap_or(exe);

    #[cfg(target_os = "macos")]
    {
        // If we live inside an .app bundle (path .../Foo.app/Contents/MacOS/Foo),
        // return the .app bundle path — that's the codesign unit.
        let mut walk = canonical.as_path();
        while let Some(parent) = walk.parent() {
            if parent.extension().map(|e| e == "app").unwrap_or(false) {
                return alloc_string(&parent.to_string_lossy());
            }
            walk = parent;
        }
    }

    alloc_string(&canonical.to_string_lossy())
}

/// Sibling backup path: `<exe>.prev` (or `<bundle>.app.prev` on macOS).
#[no_mangle]
pub extern "C" fn perry_updater_get_backup_path() -> *mut StringHeader {
    let p = perry_updater_get_exe_path();
    if p.is_null() {
        return alloc_string("");
    }
    let exe_str = unsafe {
        let len = (*p).byte_len as usize;
        let data = (p as *const u8).add(std::mem::size_of::<StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);
        std::str::from_utf8(bytes).unwrap_or("").to_string()
    };
    if exe_str.is_empty() {
        return alloc_string("");
    }
    alloc_string(&format!("{}.prev", exe_str))
}

/// Per-OS user-writable state directory + sentinel file path.
///
/// macOS: `~/Library/Application Support/<app>/updater.sentinel`
/// Windows: `%LOCALAPPDATA%\<app>\updater.sentinel`
/// Linux: `$XDG_STATE_HOME/<app>/updater.sentinel` (default: `~/.local/state`)
///
/// `<app>` is read from the `PERRY_APP_ID` env var if set; otherwise falls
/// back to the basename of the running exe. Apps that ship the updater
/// SHOULD set `PERRY_APP_ID` at compile/launch time so the path is stable
/// across renames.
#[no_mangle]
pub extern "C" fn perry_updater_get_sentinel_path() -> *mut StringHeader {
    let app = app_id();

    let dir: std::path::PathBuf;
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        dir = std::path::PathBuf::from(home)
            .join("Library/Application Support")
            .join(&app);
    }
    #[cfg(target_os = "windows")]
    {
        let local = std::env::var("LOCALAPPDATA")
            .or_else(|_| std::env::var("APPDATA"))
            .unwrap_or_else(|_| "C:\\Temp".into());
        dir = std::path::PathBuf::from(local).join(&app);
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let state = std::env::var("XDG_STATE_HOME").unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            format!("{}/.local/state", home)
        });
        dir = std::path::PathBuf::from(state).join(&app);
    }
    // No per-user state directory convention (wasm32 WASI).
    #[cfg(not(any(unix, windows)))]
    {
        dir = std::env::temp_dir().join(&app);
    }

    alloc_string(&dir.join("updater.sentinel").to_string_lossy())
}

fn app_id() -> String {
    if let Ok(id) = std::env::var("PERRY_APP_ID") {
        if !id.is_empty() {
            return id;
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(stem) = exe.file_stem() {
            return stem.to_string_lossy().to_string();
        }
    }
    "perry-app".to_string()
}

// ============================================================================
// Atomic install
// ============================================================================

/// Publish a desktop update with a completed `<target>.prev` backup first.
/// Regular files use one POSIX rename; macOS directories use a native swap.
/// Unsupported platforms/filesystems fail without removing the live target.
/// This is namespace atomicity, not power-loss durability or a snapshot of
/// concurrently modified bundles. See the updater guide for recovery states.
/// Returns 1 after publication, even if subsequent cleanup fails, or 0 on error.
#[no_mangle]
pub extern "C" fn perry_updater_install(staged_path_val: i64, target_path_val: i64) -> i64 {
    let staged = match unsafe { extract_str(staged_path_val) } {
        Some(s) => s,
        None => return 0,
    };
    let target = match unsafe { extract_str(target_path_val) } {
        Some(s) => s,
        None => return 0,
    };
    i64::from(install_with(&staged, &target, &mut publish_path))
}

fn install_with<F>(staged: &str, target: &str, publish: &mut F) -> bool
where
    F: FnMut(&str, &str) -> std::io::Result<()>,
{
    install_with_io(staged, target, publish, &mut snapshot_backup)
}

fn install_with_io<F, B>(staged: &str, target: &str, publish: &mut F, backup: &mut B) -> bool
where
    F: FnMut(&str, &str) -> std::io::Result<()>,
    B: FnMut(&str, &std::path::Path) -> std::io::Result<()>,
{
    let Ok((staged, target, prev)) = install_paths(staged, target) else {
        return false;
    };
    let mut scratch = None;
    if std::path::Path::new(&target).exists() {
        let Ok(prepared) = PreparedBackup::new(&target) else {
            return false;
        };
        let snapshot = prepared.0.join("snapshot");
        if backup(&target, &snapshot).is_err()
            || publish(snapshot.to_str().unwrap(), &prev).is_err()
        {
            return false;
        }
        scratch = Some(prepared);
    }

    // Execute permission must be in place before the new entry becomes live.
    #[cfg(unix)]
    let original_permissions = {
        use std::os::unix::fs::PermissionsExt;
        if std::path::Path::new(&staged).is_file() {
            let Ok(metadata) = std::fs::metadata(&staged) else {
                return false;
            };
            let permissions = metadata.permissions();
            if std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755)).is_err() {
                return false;
            }
            Some(permissions)
        } else {
            None
        }
    };
    if publish(&staged, &target).is_err() {
        #[cfg(unix)]
        if let Some(permissions) = original_permissions {
            let _ = std::fs::set_permissions(&staged, permissions);
        }
        return false;
    }
    // A directory exchange leaves the old tree at staged. Backup already
    // exists at prev, so interruption or cleanup failure cannot strand it.
    let _ = remove_path(&staged);
    drop(scratch);
    true
}

/// Restore a completed backup without first removing the live target.
/// Windows and uncertified directory protocols return 0 without mutation.
#[no_mangle]
pub extern "C" fn perry_updater_perform_rollback(target_path_val: i64) -> i64 {
    let target = match unsafe { extract_str(target_path_val) } {
        Some(s) => s,
        None => return 0,
    };
    i64::from(perform_rollback_with(&target, &mut publish_path))
}

fn perform_rollback_with<F>(target: &str, publish: &mut F) -> bool
where
    F: FnMut(&str, &str) -> std::io::Result<()>,
{
    let prev = format!("{}.prev", target);
    let Ok((prev, target)) = replacement_paths(&prev, target) else {
        return false;
    };
    if publish(&prev, &target).is_err() {
        return false;
    }
    // A successful directory swap puts the broken version at prev. Cleanup
    // is not part of the publication result and may be retried manually.
    let _ = remove_path(&prev);
    true
}

fn invalid_paths() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        "unsupported updater paths",
    )
}

fn normalized_path(path: &str) -> std::io::Result<std::path::PathBuf> {
    let path = std::path::Path::new(path);
    let name = path.file_name().ok_or_else(invalid_paths)?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    Ok(parent.canonicalize()?.join(name))
}

fn checked_metadata(path: &std::path::Path) -> std::io::Result<Option<std::fs::Metadata>> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() || meta.is_dir() => Ok(Some(meta)),
        Ok(_) => Err(invalid_paths()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err),
    }
}

fn replacement_paths(source: &str, target: &str) -> std::io::Result<(String, String)> {
    // Do not probe replacement alternatives that could remove a mapped EXE.
    if !cfg!(any(target_os = "macos", target_os = "linux")) {
        return Err(invalid_paths());
    }
    let source = normalized_path(source)?;
    let target = normalized_path(target)?;
    let src = checked_metadata(&source)?.ok_or_else(invalid_paths)?;
    let dst = checked_metadata(&target)?;
    if source == target
        || source.starts_with(&target)
        || target.starts_with(&source)
        || (src.is_dir() && !cfg!(target_os = "macos"))
        || dst.as_ref().is_some_and(|m| m.is_dir() != src.is_dir())
    {
        return Err(invalid_paths());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let parent = std::fs::metadata(target.parent().ok_or_else(invalid_paths)?)?;
        if src.dev() != parent.dev()
            || dst
                .as_ref()
                .is_some_and(|m| m.dev() != src.dev() || m.ino() == src.ino())
        {
            return Err(invalid_paths());
        }
    }
    Ok((
        source.to_str().ok_or_else(invalid_paths)?.to_owned(),
        target.to_str().ok_or_else(invalid_paths)?.to_owned(),
    ))
}

fn install_paths(staged: &str, target: &str) -> std::io::Result<(String, String, String)> {
    let (staged, target) = replacement_paths(staged, target)?;
    let prev = format!("{}.prev", target);
    let prev_meta = checked_metadata(std::path::Path::new(&prev))?;
    let stage_meta = std::fs::metadata(&staged)?;
    if staged == prev
        || std::path::Path::new(&staged).starts_with(&prev)
        || std::path::Path::new(&prev).starts_with(&staged)
        || prev_meta
            .as_ref()
            .is_some_and(|m| m.is_dir() != stage_meta.is_dir())
    {
        return Err(invalid_paths());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if prev_meta
            .as_ref()
            .is_some_and(|m| m.dev() != stage_meta.dev() || m.ino() == stage_meta.ino())
        {
            return Err(invalid_paths());
        }
    }
    Ok((staged, target, prev))
}

// An exclusively-created sibling directory owns only this attempt's scratch.
// An interrupted process can leave it behind, but never an incomplete prev.
struct PreparedBackup(std::path::PathBuf);

impl PreparedBackup {
    fn new(target: &str) -> std::io::Result<Self> {
        static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let sequence = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::path::PathBuf::from(format!(
            "{}.prev.prepare-{}-{}",
            target,
            std::process::id(),
            sequence
        ));
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }
}

impl Drop for PreparedBackup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn snapshot_backup(target: &str, snapshot: &std::path::Path) -> std::io::Result<()> {
    if std::path::Path::new(target).is_file() {
        return std::fs::hard_link(target, snapshot);
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        unsafe extern "C" {
            fn copyfile(
                from: *const std::ffi::c_char,
                to: *const std::ffi::c_char,
                state: *mut std::ffi::c_void,
                flags: u32,
            ) -> i32;
        }
        let from = std::ffi::CString::new(target)?;
        let to = std::ffi::CString::new(snapshot.as_os_str().as_bytes())?;
        // copyfile.h: ALL includes stat/ACL/xattrs/data; recursive clone
        // preserves symlinks and requires a nonexistent snapshot destination.
        let flags = 0xf | (1 << 15) | (1 << 24);
        // SAFETY: NUL-terminated paths outlive the call; null requests no state.
        if unsafe { copyfile(from.as_ptr(), to.as_ptr(), std::ptr::null_mut(), flags) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    Err(invalid_paths())
}

fn publish_path(from: &str, to: &str) -> std::io::Result<()> {
    if std::path::Path::new(from).is_dir() && std::path::Path::new(to).exists() {
        #[cfg(target_os = "macos")]
        {
            unsafe extern "C" {
                fn renamex_np(
                    from: *const std::ffi::c_char,
                    to: *const std::ffi::c_char,
                    flags: u32,
                ) -> i32;
            }
            let from = std::ffi::CString::new(from)?;
            let to = std::ffi::CString::new(to)?;
            // SAFETY: both C strings are live; RENAME_SWAP is 2 in sys/stdio.h.
            if unsafe { renamex_np(from.as_ptr(), to.as_ptr(), 2) } != 0 {
                return Err(std::io::Error::last_os_error());
            }
            return Ok(());
        }
        #[cfg(not(target_os = "macos"))]
        return Err(invalid_paths());
    }
    std::fs::rename(from, to)
}

fn remove_path(p: &str) -> std::io::Result<()> {
    let path = std::path::Path::new(p);
    if !path.exists() {
        return Ok(());
    }
    if path.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

// ============================================================================
// Detached relaunch
// ============================================================================

/// Spawn `exe_path` as a detached child process and return the child's PID,
/// or -1.0 on failure. The caller is expected to call `process.exit(0)`
/// shortly after — that's how the running process hands off to the new one.
///
/// No args are passed in v1; callers that need argv must wait until the
/// runtime exposes a NaN-boxed array reader here. For most autoupdate flows
/// the relaunched binary does not need extra args (it inspects its own
/// sentinel file at startup).
#[no_mangle]
pub extern "C" fn perry_updater_relaunch(exe_path_val: i64) -> f64 {
    let exe = match unsafe { extract_str(exe_path_val) } {
        Some(s) => s,
        None => return -1.0,
    };

    // Reuse the runtime's shared detached-spawn helper rather than
    // duplicating the per-OS setsid / DETACHED_PROCESS dance. Keeps the
    // detach behavior (and its quirks) in exactly one place.
    match perry_runtime::child_process::spawn_detached_command(&exe, &[], None) {
        Some(pid) => pid as f64,
        None => -1.0,
    }
}

#[cfg(test)]
#[path = "desktop_tests.rs"]
mod tests;
