use super::{host_target_triple, locate_native_lib_artifact};
use std::fs;

/// Refs #564 — when cargo writes to `target/<triple>/release/`
/// (because something pinned a default target), perry must still
/// find the artifact for a native build (no `--target` passed).
#[test]
fn locates_artifact_under_host_triple_dir_for_native_build() {
    let host = match host_target_triple() {
        Some(h) => h,
        None => return, // rustc unavailable in this test env — skip.
    };

    let tmp = tempfile::tempdir().expect("create tmpdir");
    let target_dir = tmp.path().join("target");
    let triple_dir = target_dir.join(host).join("release");
    fs::create_dir_all(&triple_dir).expect("mkdir triple/release");
    let lib_path = triple_dir.join("libfoo.a");
    fs::write(&lib_path, b"fake archive").expect("write lib");

    let found = locate_native_lib_artifact(&target_dir, None, "libfoo.a");
    assert_eq!(found.as_deref(), Some(lib_path.as_path()));
}

#[test]
fn prefers_bare_release_dir_when_present() {
    let tmp = tempfile::tempdir().expect("create tmpdir");
    let target_dir = tmp.path().join("target");
    let release_dir = target_dir.join("release");
    fs::create_dir_all(&release_dir).expect("mkdir release");
    let lib_path = release_dir.join("libfoo.a");
    fs::write(&lib_path, b"fake archive").expect("write lib");

    let found = locate_native_lib_artifact(&target_dir, None, "libfoo.a");
    assert_eq!(found.as_deref(), Some(lib_path.as_path()));
}

/// Refs #792 — wrappers that supply only the cargo crate name
/// (e.g. `perry_ext_foo`) instead of the full filename should
/// still resolve to `libperry_ext_foo.a` on the host platform.
#[test]
fn locates_artifact_from_bare_crate_name() {
    let tmp = tempfile::tempdir().expect("create tmpdir");
    let target_dir = tmp.path().join("target");
    let release_dir = target_dir.join("release");
    fs::create_dir_all(&release_dir).expect("mkdir release");
    let lib_name = if cfg!(target_os = "windows") {
        "perry_ext_foo.lib"
    } else {
        "libperry_ext_foo.a"
    };
    let lib_path = release_dir.join(lib_name);
    fs::write(&lib_path, b"fake archive").expect("write lib");

    let found = locate_native_lib_artifact(&target_dir, None, "perry_ext_foo");
    assert_eq!(found.as_deref(), Some(lib_path.as_path()));
}

/// On MSVC, cargo emits `{crate_name}.lib` literally — there is no
/// automatic `lib` prefix. So a crate whose name actually starts
/// with `lib` (e.g. `libfoo`) produces `libfoo.lib`, and Perry's
/// variant logic must NOT strip the `lib` prefix in that case.
#[test]
#[cfg(target_os = "windows")]
fn windows_preserves_lib_prefix_when_crate_name_starts_with_lib() {
    let tmp = tempfile::tempdir().expect("create tmpdir");
    let target_dir = tmp.path().join("target");
    let release_dir = target_dir.join("release");
    fs::create_dir_all(&release_dir).expect("mkdir release");
    let lib_path = release_dir.join("libfoo.lib");
    fs::write(&lib_path, b"fake archive").expect("write lib");

    let found = locate_native_lib_artifact(&target_dir, None, "libfoo");
    assert_eq!(found.as_deref(), Some(lib_path.as_path()));
}

/// Cross-platform check on the variant set itself so non-Windows
/// CI also covers the `lib`-prefix preservation logic.
#[test]
fn variant_set_for_windows_target_keeps_lib_prefix() {
    let variants = super::lib_name_variants("libfoo", Some("windows"));
    assert!(
        variants.iter().any(|v| v == "libfoo.lib"),
        "expected libfoo.lib in {:?}",
        variants
    );
}

/// Refs #5812 — a wrapper that hard-codes the Unix static-lib
/// filename (`libperry_ext_webgpu.a`) in its manifest must still
/// resolve when targeting Windows, where cargo emits
/// `perry_ext_webgpu.lib` (MSVC drops the `lib` prefix and the
/// extension is `.lib`). The literal `.a` name is still tried first
/// so Unix builds keep working unchanged.
#[test]
fn variant_set_translates_unix_static_lib_to_msvc_on_windows() {
    let variants = super::lib_name_variants("libperry_ext_webgpu.a", Some("windows"));
    assert_eq!(
        variants.first().map(String::as_str),
        Some("libperry_ext_webgpu.a")
    );
    assert!(
        variants.iter().any(|v| v == "perry_ext_webgpu.lib"),
        "expected perry_ext_webgpu.lib in {:?}",
        variants
    );
}

/// The `.a` → `.lib` translation is Windows-only; a Unix target must
/// not start probing for `.lib` files.
#[test]
fn variant_set_does_not_translate_static_lib_on_unix() {
    let variants = super::lib_name_variants("libperry_ext_webgpu.a", Some("linux"));
    assert_eq!(variants, vec!["libperry_ext_webgpu.a".to_string()]);
}

/// End-to-end: a manifest declaring `libperry_ext_webgpu.a` resolves
/// the on-disk `perry_ext_webgpu.lib` that cargo actually produced on
/// Windows. Refs #5812.
#[test]
#[cfg(target_os = "windows")]
fn locates_msvc_artifact_from_unix_manifest_name() {
    let tmp = tempfile::tempdir().expect("create tmpdir");
    let target_dir = tmp.path().join("target");
    let release_dir = target_dir.join("release");
    fs::create_dir_all(&release_dir).expect("mkdir release");
    let lib_path = release_dir.join("perry_ext_webgpu.lib");
    fs::write(&lib_path, b"fake archive").expect("write lib");

    let found = locate_native_lib_artifact(&target_dir, None, "libperry_ext_webgpu.a");
    assert_eq!(found.as_deref(), Some(lib_path.as_path()));
}

#[test]
fn returns_none_when_artifact_missing() {
    let tmp = tempfile::tempdir().expect("create tmpdir");
    let target_dir = tmp.path().join("target");
    fs::create_dir_all(&target_dir).expect("mkdir target");
    let found = locate_native_lib_artifact(&target_dir, None, "libfoo.a");
    assert!(found.is_none());
}
