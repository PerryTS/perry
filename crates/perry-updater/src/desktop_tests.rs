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
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn install_and_rollback_roundtrip() {
    let dir = std::env::temp_dir().join(format!("perry-updater-install-{}", std::process::id()));
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
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn rollback_after_committed_swap_cleans_up_without_swapping_again() {
    let (_dir, target, stage, _prev) = protocol_fixture(false);
    assert_eq!(perry_updater_install(make_str(&stage), make_str(&target)), 1);
    assert_eq!(read_version(&target, false), b"new");
    assert_eq!(perry_updater_perform_rollback(make_str(&target)), 1);
    assert_eq!(read_version(&target, false), b"old");

    // Simulate a crash after a committed rollback whose cleanup never ran:
    // the broken version sits at .prev and the rollback marker is armed.
    let prev = format!("{}.prev", target);
    let marker = format!("{}.rollback", prev);
    std::fs::write(&prev, b"new").unwrap();
    std::fs::write(&marker, b"").unwrap();

    // The second call reports the earlier publication and tolerates the
    // stale backup instead of deleting it: prev may still be the only good
    // copy when the marker was armed before the swap committed.
    assert_eq!(perry_updater_perform_rollback(make_str(&target)), 1);
    assert_eq!(read_version(&target, false), b"old");
    assert_eq!(std::fs::read(&prev).unwrap(), b"new");
    assert!(std::path::Path::new(&marker).exists());
}

#[test]
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn rollback_with_armed_marker_never_deletes_the_backup() {
    let (_dir, target, _stage, _prev) = protocol_fixture(false);
    // Crash window: the marker is armed before the swap commits, so prev
    // still holds the good backup while target holds the broken version.
    let prev = format!("{}.prev", target);
    let marker = format!("{}.rollback", prev);
    std::fs::write(&prev, b"good").unwrap();
    std::fs::write(&target, b"broken").unwrap();
    std::fs::write(&marker, b"").unwrap();

    assert_eq!(perry_updater_perform_rollback(make_str(&target)), 1);
    assert_eq!(std::fs::read(&prev).unwrap(), b"good");
    assert!(std::path::Path::new(&marker).exists());
    assert_eq!(std::fs::read(&target).unwrap(), b"broken");
}

#[test]
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn install_clears_armed_rollback_marker() {
    let (_dir, target, stage, _prev) = protocol_fixture(false);
    let marker = format!("{}.prev.rollback", target);
    // A previous rollback left the marker armed after a failed cleanup.
    std::fs::write(&marker, b"").unwrap();

    assert_eq!(perry_updater_install(make_str(&stage), make_str(&target)), 1);
    assert_eq!(read_version(&target, false), b"new");
    assert!(!std::path::Path::new(&marker).exists());

    // The next rollback swaps again instead of reporting a stale success.
    assert_eq!(perry_updater_perform_rollback(make_str(&target)), 1);
    assert_eq!(read_version(&target, false), b"old");
}

#[test]
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn rollback_normalizes_trailing_separator_target() {
    let (_dir, target, stage, _prev) = protocol_fixture(false);
    assert_eq!(perry_updater_install(make_str(&stage), make_str(&target)), 1);
    assert_eq!(read_version(&target, false), b"new");

    // A trailing separator must not derive .prev inside the target, where
    // the backup cannot resolve and rollback would fail with the backup
    // intact at the sibling path.
    let trailing = format!("{}/", target);
    assert_eq!(perry_updater_perform_rollback(make_str(&trailing)), 1);
    assert_eq!(read_version(&target, false), b"old");
    assert!(!std::path::Path::new(&format!("{}.prev", target)).exists());
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
    let prev_for_failure = normalized_path(&prev).unwrap().to_str().unwrap().to_owned();
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

#[test]
fn rejected_missing_stage_preserves_all_paths() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("app");
    let prev = dir.path().join("app.prev");
    let staged = dir.path().join("missing");
    std::fs::write(&target, b"current").unwrap();
    std::fs::write(&prev, b"previous").unwrap();
    assert_eq!(
        perry_updater_install(
            make_str(staged.to_str().unwrap()),
            make_str(target.to_str().unwrap())
        ),
        0
    );
    assert_eq!(std::fs::read(&target).unwrap(), b"current");
    assert_eq!(std::fs::read(&prev).unwrap(), b"previous");
    assert!(!staged.exists());
}

#[test]
fn rejected_backup_as_stage_preserves_all_paths() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("app");
    let prev = dir.path().join("app.prev");
    std::fs::write(&target, b"current").unwrap();
    std::fs::write(&prev, b"previous").unwrap();
    assert_eq!(
        perry_updater_install(
            make_str(prev.to_str().unwrap()),
            make_str(target.to_str().unwrap())
        ),
        0
    );
    assert_eq!(std::fs::read(&target).unwrap(), b"current");
    assert_eq!(std::fs::read(&prev).unwrap(), b"previous");
}

fn fixture_hash(root: &std::path::Path) -> Vec<u8> {
    fixture_hash_with_metadata(root, &|_, _| Vec::new())
}

fn fixture_hash_with_metadata<F>(root: &std::path::Path, metadata: &F) -> Vec<u8>
where
    F: Fn(&std::path::Path, &std::fs::Metadata) -> Vec<u8>,
{
    use sha2::{Digest, Sha256};
    fn walk<F>(root: &std::path::Path, path: &std::path::Path, hash: &mut Sha256, metadata: &F)
    where
        F: Fn(&std::path::Path, &std::fs::Metadata) -> Vec<u8>,
    {
        let meta = std::fs::symlink_metadata(path).unwrap();
        hash.update(metadata(path, &meta));
        hash.update(
            path.strip_prefix(root)
                .unwrap()
                .as_os_str()
                .as_encoded_bytes(),
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            hash.update(meta.permissions().mode().to_le_bytes());
        }
        if meta.is_symlink() {
            hash.update(b"link");
            hash.update(
                std::fs::read_link(path)
                    .unwrap()
                    .as_os_str()
                    .as_encoded_bytes(),
            );
        } else if meta.is_dir() {
            hash.update(b"dir");
            let mut children: Vec<_> = std::fs::read_dir(path)
                .unwrap()
                .map(|e| e.unwrap().path())
                .collect();
            children.sort();
            for child in children {
                walk(root, &child, hash, metadata);
            }
        } else {
            hash.update(b"file");
            hash.update(std::fs::read(path).unwrap());
        }
    }
    let mut hash = Sha256::new();
    walk(root, root, &mut hash, metadata);
    hash.finalize().to_vec()
}

#[test]
fn rejected_aliases_and_types_leave_fixture_hash_unchanged() {
    for case in [
        "same",
        "hardlink",
        "backup",
        "directory",
        "nested",
        "symlink",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("app");
        let staged = dir.path().join("stage");
        let prev = dir.path().join("app.prev");
        std::fs::write(&target, b"old").unwrap();
        std::fs::write(&prev, b"previous").unwrap();
        let source = match case {
            "same" => target.clone(),
            "backup" => prev.clone(),
            "hardlink" => {
                std::fs::hard_link(&target, &staged).unwrap();
                staged.clone()
            }
            "directory" => {
                std::fs::create_dir(&staged).unwrap();
                staged.clone()
            }
            "nested" => {
                std::fs::remove_file(&target).unwrap();
                std::fs::create_dir(&target).unwrap();
                let child = target.join("child");
                std::fs::create_dir(&child).unwrap();
                child
            }
            _ => {
                #[cfg(unix)]
                std::os::unix::fs::symlink(&target, &staged).unwrap();
                #[cfg(not(unix))]
                {
                    continue;
                }
                staged.clone()
            }
        };
        let before = fixture_hash(dir.path());
        assert_eq!(
            perry_updater_install(
                make_str(source.to_str().unwrap()),
                make_str(target.to_str().unwrap())
            ),
            0,
            "{case}"
        );
        assert_eq!(fixture_hash(dir.path()), before, "{case}");
    }
}

#[test]
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn initial_install_without_target_keeps_existing_backup() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("app");
    let staged = dir.path().join("stage");
    let prev = dir.path().join("app.prev");
    std::fs::write(&staged, b"new").unwrap();
    std::fs::write(&prev, b"previous").unwrap();
    assert_eq!(
        perry_updater_install(
            make_str(staged.to_str().unwrap()),
            make_str(target.to_str().unwrap())
        ),
        1
    );
    assert_eq!(std::fs::read(&target).unwrap(), b"new");
    assert_eq!(std::fs::read(&prev).unwrap(), b"previous");
    assert!(!staged.exists());
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn protocol_fixture(bundle: bool) -> (tempfile::TempDir, String, String, String) {
    let dir = tempfile::tempdir().unwrap();
    for (name, bytes) in [
        ("app", b"old".as_slice()),
        ("stage", b"new".as_slice()),
        ("app.prev", b"previous".as_slice()),
    ] {
        let path = dir.path().join(name);
        if bundle {
            std::fs::create_dir(&path).unwrap();
            std::fs::write(path.join("binary"), bytes).unwrap();
            #[cfg(unix)]
            std::os::unix::fs::symlink("binary", path.join("alias")).unwrap();
        } else {
            std::fs::write(path, bytes).unwrap();
        }
    }
    let root = dir.path().canonicalize().unwrap();
    let target = root.join("app").to_str().unwrap().to_owned();
    let stage = root.join("stage").to_str().unwrap().to_owned();
    let prev = root.join("app.prev").to_str().unwrap().to_owned();
    (dir, target, stage, prev)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn read_version(path: &str, bundle: bool) -> Vec<u8> {
    std::fs::read(if bundle {
        std::path::Path::new(path).join("binary")
    } else {
        path.into()
    })
    .unwrap()
}

#[test]
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn interruption_at_each_publication_keeps_live_target_and_completed_backup() {
    let kinds = if cfg!(target_os = "macos") {
        vec![false, true]
    } else {
        vec![false]
    };
    for bundle in kinds {
        for (destination, after) in [
            ("backup", false),
            ("backup", true),
            ("target", false),
            ("target", true),
        ] {
            let (_dir, target, stage, prev) = protocol_fixture(bundle);
            let child = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "-Z",
                    "unstable-options",
                    "--force-run-in-process",
                    "--exact",
                    "desktop::tests::interruption_child",
                    "--test-threads=1",
                ])
                .env("PERRY_UPDATER_INTERRUPT_FIXTURE", _dir.path())
                .env("PERRY_UPDATER_INTERRUPT_DEST", destination)
                .env(
                    "PERRY_UPDATER_INTERRUPT_AFTER",
                    if after { "1" } else { "0" },
                )
                .output()
                .unwrap();
            assert_eq!(
                child.status.code(),
                Some(71),
                "{}",
                String::from_utf8_lossy(&child.stderr)
            );
            let committed = destination == "target" && after;
            assert_eq!(
                read_version(&target, bundle),
                if committed { b"new" } else { b"old" }
            );
            assert_eq!(
                read_version(&prev, bundle),
                if destination == "backup" && !after {
                    b"previous".as_slice()
                } else {
                    b"old".as_slice()
                }
            );
            if committed && bundle {
                assert_eq!(read_version(&stage, bundle), b"old");
            }
        }
    }
}

// A separate test process exits without unwinding, so scratch Drop and any
// in-process error recovery cannot hide an interrupted publication state.
#[test]
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn interruption_child() {
    let Some(root) = std::env::var_os("PERRY_UPDATER_INTERRUPT_FIXTURE") else {
        return;
    };
    let root = std::path::Path::new(&root).canonicalize().unwrap();
    let target = root.join("app").to_str().unwrap().to_owned();
    let stage = root.join("stage").to_str().unwrap().to_owned();
    let prev = root.join("app.prev").to_str().unwrap().to_owned();
    let destination = std::env::var("PERRY_UPDATER_INTERRUPT_DEST").unwrap();
    let after = std::env::var("PERRY_UPDATER_INTERRUPT_AFTER").unwrap() == "1";
    let mut publish = |from: &str, to: &str| {
        assert!(
            std::path::Path::new(&target).exists(),
            "live path disappeared"
        );
        let interrupt = if destination == "target" {
            to == target
        } else {
            to == prev
        };
        if interrupt && !after {
            std::process::exit(71);
        }
        publish_path(from, to)?;
        if interrupt {
            std::process::exit(71);
        }
        Ok(())
    };
    assert!(install_with(&stage, &target, &mut publish));
    panic!("interruption point was not reached");
}

#[test]
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn failed_copy_does_not_replace_live_target_or_existing_backup() {
    let (dir, target, stage, _prev) = protocol_fixture(false);
    let before = fixture_hash(dir.path());
    let mut partial_copy = |_: &str, path: &std::path::Path| {
        std::fs::write(path, b"incomplete")?;
        Err(std::io::Error::other("injected incomplete backup"))
    };
    assert!(!install_with_io(
        &stage,
        &target,
        &mut publish_path,
        &mut partial_copy
    ));
    assert_eq!(fixture_hash(dir.path()), before);
}

#[test]
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn failed_publication_preserves_live_target_and_rollback_backup() {
    for error in [
        std::io::ErrorKind::CrossesDevices,
        std::io::ErrorKind::Unsupported,
        std::io::ErrorKind::PermissionDenied,
    ] {
        for fail_backup in [false, true] {
            let (dir, target, stage, prev) = protocol_fixture(false);
            let before = fixture_hash(dir.path());
            let mut publish = |from: &str, to: &str| {
                if to == if fail_backup { &prev } else { &target } {
                    return Err(std::io::Error::new(error, "injected rename error"));
                }
                publish_path(from, to)
            };
            assert!(!install_with(&stage, &target, &mut publish));
            assert_eq!(read_version(&target, false), b"old");
            assert_eq!(read_version(&stage, false), b"new");
            assert_eq!(
                read_version(&prev, false),
                if fail_backup {
                    b"previous".as_slice()
                } else {
                    b"old".as_slice()
                }
            );
            if fail_backup {
                assert_eq!(fixture_hash(dir.path()), before);
            }
            assert!(
                perform_rollback_with(&target, &mut publish_path)
                    || read_version(&target, false) == b"old"
            );
        }
    }
}

#[test]
#[cfg(target_os = "macos")]
fn bundle_backup_preserves_metadata_and_rollback_replaces_nonempty_tree() {
    use std::os::unix::fs::PermissionsExt;
    let (_dir, target, stage, prev) = protocol_fixture(true);
    let value = b"metadata";
    set_fixture_xattr(std::path::Path::new(&format!("{target}/binary")), value);
    std::fs::set_permissions(
        format!("{target}/binary"),
        std::fs::Permissions::from_mode(0o751),
    )
    .unwrap();
    assert_eq!(
        perry_updater_install(make_str(&stage), make_str(&target)),
        1
    );
    assert_eq!(read_version(&target, true), b"new");
    assert_eq!(read_version(&prev, true), b"old");
    let copied = fixture_xattr(std::path::Path::new(&format!("{prev}/binary")));
    assert_eq!(copied.len(), 8);
    assert_eq!(copied, value);
    assert_eq!(
        std::fs::read_link(format!("{prev}/alias")).unwrap(),
        std::path::Path::new("binary")
    );
    assert_eq!(
        std::fs::metadata(format!("{prev}/binary"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o751
    );
    assert!(!std::path::Path::new(&stage).exists());
    assert_eq!(perry_updater_perform_rollback(make_str(&target)), 1);
    assert_eq!(read_version(&target, true), b"old");
    assert!(!std::path::Path::new(&prev).exists());
}

#[test]
#[cfg(target_os = "macos")]
fn cleanup_failure_after_directory_publication_still_reports_success() {
    let (_dir, target, stage, prev) = protocol_fixture(true);
    let mut publish = |from: &str, to: &str| {
        publish_path(from, to)?;
        if to == target {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(from, std::fs::Permissions::from_mode(0o555))?;
            assert!(remove_path(from).is_err(), "fixture must prevent cleanup");
        }
        Ok(())
    };
    let installed = install_with(&stage, &target, &mut publish);
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&stage, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(installed);
    assert_eq!(read_version(&target, true), b"new");
    assert_eq!(read_version(&prev, true), b"old");
}

#[test]
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn unsupported_platform_install_and_rollback_preserve_fixture_hash() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("app");
    let stage = dir.path().join("stage");
    std::fs::write(&target, b"old").unwrap();
    std::fs::write(&stage, b"new").unwrap();
    std::fs::write(dir.path().join("app.prev"), b"previous").unwrap();
    let before = fixture_hash(dir.path());
    assert_eq!(
        perry_updater_install(
            make_str(stage.to_str().unwrap()),
            make_str(target.to_str().unwrap())
        ),
        0
    );
    assert_eq!(
        perry_updater_perform_rollback(make_str(target.to_str().unwrap())),
        0
    );
    assert_eq!(fixture_hash(dir.path()), before);
}

#[test]
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn rollback_publication_failure_keeps_both_versions() {
    let kinds = if cfg!(target_os = "macos") {
        vec![false, true]
    } else {
        vec![false]
    };
    for bundle in kinds {
        let (dir, target, _stage, _prev) = protocol_fixture(bundle);
        let before = fixture_hash(dir.path());
        let mut fail = |_: &str, _: &str| {
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "injected unsupported publication",
            ))
        };
        assert!(!perform_rollback_with(&target, &mut fail));
        assert_eq!(fixture_hash(dir.path()), before);
    }
}

#[test]
#[cfg(target_os = "linux")]
fn linux_directory_install_and_rollback_leave_fixture_unchanged() {
    let (dir, target, stage, _prev) = protocol_fixture(true);
    let before = fixture_hash(dir.path());
    assert_eq!(
        perry_updater_install(make_str(&stage), make_str(&target)),
        0
    );
    assert_eq!(perry_updater_perform_rollback(make_str(&target)), 0);
    assert_eq!(fixture_hash(dir.path()), before);
}

#[cfg(target_os = "macos")]
fn set_fixture_xattr(path: &std::path::Path, value: &[u8]) {
    unsafe extern "C" {
        fn setxattr(
            path: *const std::ffi::c_char,
            name: *const std::ffi::c_char,
            value: *const std::ffi::c_void,
            size: usize,
            position: u32,
            options: i32,
        ) -> i32;
    }
    let path = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
    // SAFETY: the C strings and byte slice outlive this synchronous call.
    assert_eq!(
        unsafe {
            setxattr(
                path.as_ptr(),
                c"user.perry-backup-test".as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
                0,
            )
        },
        0
    );
}

#[cfg(target_os = "macos")]
fn fixture_xattr(path: &std::path::Path) -> Vec<u8> {
    unsafe extern "C" {
        fn getxattr(
            path: *const std::ffi::c_char,
            name: *const std::ffi::c_char,
            value: *mut std::ffi::c_void,
            size: usize,
            position: u32,
            options: i32,
        ) -> isize;
    }
    let path = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
    let mut value = [0_u8; 64];
    // SAFETY: the buffer capacity matches the supplied size and paths are live.
    let size = unsafe {
        getxattr(
            path.as_ptr(),
            c"user.perry-backup-test".as_ptr(),
            value.as_mut_ptr().cast(),
            value.len(),
            0,
            0,
        )
    };
    if size < 0 {
        Vec::new()
    } else {
        value[..size as usize].to_vec()
    }
}

#[test]
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn install_and_rollback_with_existing_backup_preserves_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let (dir, target, stage, prev) = protocol_fixture(false);
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o751)).unwrap();
    assert_eq!(
        perry_updater_install(make_str(&stage), make_str(&target)),
        1
    );
    assert_eq!(read_version(&target, false), b"new");
    assert_eq!(read_version(&prev, false), b"old");
    assert!(!std::path::Path::new(&stage).exists());
    assert_eq!(
        std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
        0o755
    );
    assert_eq!(perry_updater_perform_rollback(make_str(&target)), 1);
    assert_eq!(read_version(&target, false), b"old");
    assert_eq!(
        std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
        0o751
    );
    assert!(!std::path::Path::new(&prev).exists());
    assert!(!dir.path().join("app.broken").exists());
}

#[test]
#[cfg(target_os = "macos")]
fn rejected_backup_symlink_preserves_fixture_metadata() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let (dir, target, stage, prev) = protocol_fixture(false);
    let other = dir.path().join("other");
    std::fs::write(&other, b"unrelated").unwrap();
    std::fs::remove_file(&prev).unwrap();
    std::os::unix::fs::symlink("other", &prev).unwrap();
    for path in [
        std::path::Path::new(&target),
        std::path::Path::new(&stage),
        &other,
    ] {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o751)).unwrap();
        set_fixture_xattr(path, b"metadata");
        assert_eq!(fixture_xattr(path), b"metadata");
    }
    let metadata = |path: &std::path::Path, meta: &std::fs::Metadata| {
        let mut state = Vec::new();
        for value in [
            u64::from(meta.uid()),
            u64::from(meta.gid()),
            meta.ino(),
            meta.nlink(),
        ] {
            state.extend_from_slice(&value.to_le_bytes());
        }
        state.extend_from_slice(&meta.mtime().to_le_bytes());
        state.extend_from_slice(&meta.mtime_nsec().to_le_bytes());
        if meta.is_file() {
            state.extend(fixture_xattr(path));
        }
        state
    };
    let before = fixture_hash_with_metadata(dir.path(), &metadata);
    assert_eq!(
        perry_updater_install(make_str(&stage), make_str(&target)),
        0
    );
    assert_eq!(fixture_hash_with_metadata(dir.path(), &metadata), before);
}
