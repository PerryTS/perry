use super::*;

#[test]
fn windows_sdk_defaults_and_explicit_overrides() {
    let home = Path::new("Users/Developer Name");
    let local = Path::new("Users/Developer Name/AppData/Local");
    assert_eq!(
        resolve_sdk_root(None, None, Some(home), Some(local), "windows"),
        Some(local.join("Android/Sdk"))
    );
    assert_eq!(
        resolve_sdk_root(None, None, Some(home), None, "windows"),
        Some(home.join("AppData/Local/Android/Sdk"))
    );
    assert_eq!(
        resolve_sdk_root(
            Some(OsStr::new("custom sdk")),
            Some(OsStr::new("other sdk")),
            Some(home),
            Some(local),
            "windows"
        ),
        Some(PathBuf::from("custom sdk"))
    );
    assert_eq!(
        resolve_sdk_root(
            Some(OsStr::new("")),
            Some(OsStr::new("other sdk")),
            None,
            None,
            "windows"
        ),
        Some(PathBuf::from("other sdk"))
    );
    assert_eq!(resolve_sdk_root(None, None, None, None, "windows"), None);
}

#[test]
fn unix_sdk_defaults_are_host_specific() {
    let home = Path::new("/home/developer");
    assert_eq!(
        resolve_sdk_root(None, None, Some(home), None, "macos"),
        Some(home.join("Library/Android/sdk"))
    );
    assert_eq!(
        resolve_sdk_root(None, None, Some(home), None, "linux"),
        Some(home.join("Android/Sdk"))
    );
}

#[test]
fn windows_sdk_finds_exe_and_batch_tools_in_newest_usable_version() {
    let temp = tempfile::tempdir().unwrap();
    let tools = temp.path().join("Android SDK/build-tools");
    for version in ["35.0.0", "37.0.0", "38.0.0"] {
        std::fs::create_dir_all(tools.join(version)).unwrap();
    }
    for path in [
        "35.0.0/apksigner.bat",
        "37.0.0/apksigner.bat",
        "37.0.0/zipalign.exe",
    ] {
        std::fs::write(tools.join(path), b"tool").unwrap();
    }
    // A directory with a tool's name must not count as an executable.
    std::fs::create_dir(tools.join("38.0.0/apksigner.bat")).unwrap();
    assert_eq!(
        find_latest_build_tool(&tools, "apksigner", true),
        Some(tools.join("37.0.0/apksigner.bat"))
    );
    assert_eq!(
        find_latest_build_tool(&tools, "zipalign", true),
        Some(tools.join("37.0.0/zipalign.exe"))
    );
    assert_eq!(find_latest_build_tool(&tools, "apksigner", false), None);
}

#[test]
fn unix_sdk_keeps_extensionless_tools() {
    let temp = tempfile::tempdir().unwrap();
    let tools = temp.path().join("build-tools");
    std::fs::create_dir_all(tools.join("35.0.0")).unwrap();
    let path = tools.join("35.0.0/apksigner");
    std::fs::write(&path, b"tool").unwrap();
    assert_eq!(
        find_latest_build_tool(&tools, "apksigner", false),
        Some(path)
    );
}

#[cfg(unix)]
fn script(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
#[cfg(unix)]
fn zipalign_uses_distinct_arguments_and_replaces_the_apk() {
    let temp = tempfile::tempdir().unwrap();
    let apk = temp.path().join("app & sample.apk");
    let tool = temp.path().join("fake zipalign");
    std::fs::write(&apk, b"original").unwrap();
    script(&tool, "test \"$1\" = -f || exit 1\ntest \"$2\" = 4 || exit 1\ncat \"$3\" > \"$4\"\nprintf aligned >> \"$4\"");
    align_apk(&apk, &tool).unwrap();
    assert_eq!(std::fs::read(&apk).unwrap(), b"originalaligned");
}

#[test]
#[cfg(unix)]
fn failed_zipalign_preserves_original_apk_and_reports_failure() {
    let temp = tempfile::tempdir().unwrap();
    let apk = temp.path().join("app.apk");
    let tool = temp.path().join("zipalign");
    std::fs::write(&apk, b"original").unwrap();
    script(&tool, "exit 7");
    assert!(align_apk(&apk, &tool)
        .unwrap_err()
        .to_string()
        .contains("zipalign failed"));
    assert_eq!(std::fs::read(apk).unwrap(), b"original");
}

#[cfg(unix)]
fn windows_sdk_fixture(root: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let home = root.join("Developer Home");
    let sdk = root.join("SDK & tools");
    let tools = sdk.join("build-tools/37.0.0");
    std::fs::create_dir_all(home.join(".android")).unwrap();
    std::fs::create_dir_all(&tools).unwrap();
    std::fs::write(home.join(".android/debug.keystore"), b"test key").unwrap();
    // On Unix these executable test scripts simulate Windows SDK filenames;
    // the discovery branch and full signing pipeline are the production code.
    script(
        &tools.join("zipalign.exe"),
        "cat \"$3\" > \"$4\"\nprintf aligned >> \"$4\"",
    );
    script(&tools.join("apksigner.bat"), "test \"$1\" = sign || exit 2\ntest -f \"$3\" || exit 3\ntest \"$(cat \"${10}\")\" = originalaligned || exit 4\nprintf signed > \"${10}.signed\"");
    let apk = root.join("app & sample.apk");
    std::fs::write(&apk, b"original").unwrap();
    (home, sdk, apk)
}

#[test]
#[cfg(unix)]
fn sdk_pipeline_uses_discovered_windows_tools_and_signs_aligned_input() {
    let temp = tempfile::tempdir().unwrap();
    let (home, sdk, apk) = windows_sdk_fixture(temp.path());
    assert_eq!(
        debug_sign_apk_in_sdk(&apk, OutputFormat::Json, &home, &sdk, true).unwrap(),
        apk
    );
    assert_eq!(
        std::fs::read(apk.with_extension("apk.signed")).unwrap(),
        b"signed"
    );
}

#[test]
#[cfg(unix)]
fn sdk_pipeline_does_not_sign_after_failed_or_missing_alignment() {
    let temp = tempfile::tempdir().unwrap();
    let (home, sdk, apk) = windows_sdk_fixture(temp.path());
    let tool = sdk.join("build-tools/37.0.0/zipalign.exe");
    script(&tool, "exit 7");
    assert!(
        debug_sign_apk_in_sdk(&apk, OutputFormat::Json, &home, &sdk, true)
            .unwrap_err()
            .to_string()
            .contains("zipalign failed")
    );
    std::fs::remove_file(tool).unwrap();
    assert!(
        debug_sign_apk_in_sdk(&apk, OutputFormat::Json, &home, &sdk, true)
            .unwrap_err()
            .to_string()
            .contains("zipalign not found")
    );
    assert!(!apk.with_extension("apk.signed").exists());
    assert_eq!(std::fs::read(apk).unwrap(), b"original");
}

#[test]
#[cfg(windows)]
fn apksigner_batch_launch_preserves_paths_with_spaces_and_ampersands() {
    let temp = tempfile::tempdir().unwrap();
    let signer = temp.path().join("fake apksigner.bat");
    let key = temp.path().join("debug & test.keystore");
    let apk = temp.path().join("app & sample.apk");
    std::fs::write(&key, b"key").unwrap();
    std::fs::write(&apk, b"apk").unwrap();
    // The APK is argument 10; shift to it without interpolating a command line.
    let batch = format!("@echo off\r\nif not \"%~1\"==\"sign\" exit /b 2\r\nif not exist \"%~3\" exit /b 3\r\n{}copy /y \"%~1\" \"%~1.signed\" >nul\r\n", "shift\r\n".repeat(9));
    std::fs::write(&signer, batch).unwrap();
    sign_with_apksigner(&apk, &key, &signer).unwrap();
    assert_eq!(
        std::fs::read(apk.with_extension("apk.signed")).unwrap(),
        b"apk"
    );
}
