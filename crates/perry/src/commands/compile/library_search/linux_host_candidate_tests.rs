use super::{is_native_linux_target, push_executable_relative_host_candidates};
use std::path::{Path, PathBuf};

#[test]
fn native_linux_alias_matches_only_the_host_architecture_and_libc() {
    if cfg!(all(target_arch = "x86_64", target_env = "gnu")) {
        assert!(is_native_linux_target(Some("linux")));
        assert!(is_native_linux_target(Some("linux-x86_64")));
        assert!(!is_native_linux_target(Some("linux-arm64")));
        assert!(!is_native_linux_target(Some("linux-musl")));
    } else if cfg!(all(target_arch = "aarch64", target_env = "gnu")) {
        assert!(is_native_linux_target(Some("linux-arm64")));
        assert!(!is_native_linux_target(Some("linux")));
        assert!(!is_native_linux_target(Some("linux-x86_64")));
    }
}

#[test]
fn npm_bin_layout_probes_the_sibling_lib_directory() {
    let mut candidates = Vec::new();
    push_executable_relative_host_candidates(
        &mut candidates,
        Path::new("/project/node_modules/@perryts/perry-linux-x64/bin/perry"),
        "libperry_ui_gtk4.a",
    );

    assert_eq!(
        candidates,
        [
            PathBuf::from("/project/node_modules/@perryts/perry-linux-x64/bin/libperry_ui_gtk4.a",),
            PathBuf::from("/project/node_modules/@perryts/perry-linux-x64/lib/libperry_ui_gtk4.a",),
        ]
    );
}
