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

/// Replace `target_path` with `staged_path`, keeping the displaced version at
/// `<target>.prev` for rollback. Atomic on the same filesystem on every
/// supported platform — but "atomic" means something subtly different on
/// each OS, so the live-process-survives-its-own-replacement guarantee is
/// worth spelling out:
///
/// **macOS / Linux (POSIX `rename(2)`)**: rename swaps the directory entry
/// in one syscall — the new name points at the new inode atomically.
/// Crucially, the *currently-running* process holds open file descriptors
/// (or, more precisely, an mmap'd image-section reference) to the OLD
/// inode. POSIX guarantees the inode stays alive until every reference is
/// dropped, so the running binary keeps executing happily out of the now-
/// unreachable inode while the new binary takes over the path. Same logic
/// applies to swapping a `.app` bundle on macOS — bundle swap is a
/// directory rename, not a file replace, but the running Mach-O lives
/// inside `Contents/MacOS/...` and that file's inode is held by the
/// process's image section.
///
/// **Windows (NTFS rename via `MoveFileExW`)**: NTFS allows a file to be
/// renamed even while it's mapped as a code section by the loader,
/// provided the loader opened the file with `FILE_SHARE_DELETE` — which
/// it has on every Windows version since Vista. `std::fs::rename` calls
/// `MoveFileExW(MOVEFILE_REPLACE_EXISTING)` which honors that share mode.
/// The running EXE's image section continues to reference the renamed
/// file (now at `<exe>.prev`) until process exit; the next launch sees
/// the new file at the original path.
///
/// In both cases the relaunch is what actually flips the process onto
/// the new code; the rename just makes sure the path the OS will look
/// up next time points at the new bytes.
///
/// Returns 1 on success, 0 on any IO error.
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

    let prev = format!("{}.prev", target);

    // Best-effort: remove a stale .prev from a previous update. Ignore errors
    // (Windows EBUSY on the file is acceptable — we accept the leak in MVP).
    let _ = remove_path(&prev);

    // 1) Move current target → .prev
    if std::path::Path::new(&target).exists() && rename_path(&target, &prev).is_err() {
        return 0;
    }

    // 2) Move staged → target
    if rename_path(&staged, &target).is_err() {
        // Rollback step 1: try to put .prev back.
        let _ = rename_path(&prev, &target);
        return 0;
    }

    // 3) On Unix, ensure the new binary is executable.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(&target) {
            let mut perms = meta.permissions();
            perms.set_mode(0o755);
            let _ = std::fs::set_permissions(&target, perms);
        }
    }

    1
}

/// Restore `<target>.prev` over `target`, undoing a prior install. Returns 1
/// on success, 0 if no backup exists or rename failed.
#[no_mangle]
pub extern "C" fn perry_updater_perform_rollback(target_path_val: i64) -> i64 {
    let target = match unsafe { extract_str(target_path_val) } {
        Some(s) => s,
        None => return 0,
    };

    let mut rename = rename_path;
    if perform_rollback_with(&target, &mut rename) {
        1
    } else {
        0
    }
}

/// Rollback implementation factored for deterministic failure-path tests.
/// The native ABI above supplies `rename_path`; tests can inject a failed
/// second rename and verify that the broken executable is restored.
fn perform_rollback_with<F>(target: &str, rename: &mut F) -> bool
where
    F: FnMut(&str, &str) -> std::io::Result<()>,
{
    let prev = format!("{}.prev", target);

    if !std::path::Path::new(&prev).exists() {
        return false;
    }

    // Move current (likely-broken) target out of the way.
    let broken = format!("{}.broken", target);
    let _ = remove_path(&broken);
    let _ = rename(target, &broken);

    if rename(&prev, target).is_err() {
        // Try to put broken back so the system isn't left without an exe.
        let _ = rename(&broken, target);
        return false;
    }

    let _ = remove_path(&broken);
    true
}

fn rename_path(from: &str, to: &str) -> std::io::Result<()> {
    // For directories (macOS .app bundles), `std::fs::rename` works the same
    // as for files when both ends are on the same filesystem.
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
mod tests {
    use super::*;

    fn make_str(s: &str) -> i64 {
        js_string_from_bytes(s.as_ptr(), s.len() as u32) as i64
    }

    fn read_str(p: *mut StringHeader) -> String {
        unsafe {
            if p.is_null() {
                return String::new();
            }
            let len = (*p).byte_len as usize;
            let data = (p as *const u8).add(std::mem::size_of::<StringHeader>());
            let bytes = std::slice::from_raw_parts(data, len);
            std::str::from_utf8(bytes).unwrap_or("").to_string()
        }
    }

    #[test]
    fn install_and_rollback_roundtrip() {
        let dir =
            std::env::temp_dir().join(format!("perry-updater-install-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let target = dir.join("app.bin");
        let staged = dir.join("staged.bin");

        std::fs::write(&target, b"v1").unwrap();
        std::fs::write(&staged, b"v2").unwrap();

        let target_s = target.to_string_lossy().to_string();
        let staged_s = staged.to_string_lossy().to_string();
        assert_eq!(
            perry_updater_install(make_str(&staged_s), make_str(&target_s)),
            1
        );

        assert_eq!(std::fs::read(&target).unwrap(), b"v2");
        let prev = format!("{}.prev", target_s);
        assert_eq!(std::fs::read(&prev).unwrap(), b"v1");
        assert!(!staged.exists(), "staged should have moved");

        // Now roll back.
        assert_eq!(perry_updater_perform_rollback(make_str(&target_s)), 1);
        assert_eq!(std::fs::read(&target).unwrap(), b"v1");

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rollback_without_backup_fails() {
        let dir = std::env::temp_dir().join(format!("perry-updater-rb-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("only.bin");
        std::fs::write(&target, b"x").unwrap();

        let s = target.to_string_lossy().to_string();
        assert_eq!(perry_updater_perform_rollback(make_str(&s)), 0);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rollback_rename_failure_restores_broken_target() {
        let dir = std::env::temp_dir().join(format!(
            "perry-updater-rb-rename-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("app.bin");
        let prev = format!("{}.prev", target.display());
        std::fs::write(&target, b"broken-v2").unwrap();
        std::fs::write(&prev, b"backup-v1").unwrap();

        let target_s = target.to_string_lossy().to_string();
        let prev_for_failure = prev.clone();
        let mut rename = move |from: &str, to: &str| {
            if from == prev_for_failure {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "injected rollback rename failure",
                ));
            }
            std::fs::rename(from, to)
        };

        assert!(!perform_rollback_with(&target_s, &mut rename));
        assert_eq!(std::fs::read(&target).unwrap(), b"broken-v2");
        assert_eq!(std::fs::read(&prev).unwrap(), b"backup-v1");
        assert!(!std::path::Path::new(&format!("{}.broken", target.display())).exists());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn sentinel_path_uses_app_id() {
        std::env::set_var("PERRY_APP_ID", "test-app-id-zzz");
        let p = read_str(perry_updater_get_sentinel_path());
        assert!(p.contains("test-app-id-zzz"), "sentinel path = {}", p);
        assert!(p.ends_with("updater.sentinel"));
        std::env::remove_var("PERRY_APP_ID");
    }
}
