use super::{
    collect_library_candidates, msvc_vswhere_installation_path_args, xwin_sysroot_lib_paths,
    WindowsTargetArch,
};

#[test]
fn vswhere_query_requires_msvc_tools_component() {
    assert_eq!(
        msvc_vswhere_installation_path_args(WindowsTargetArch::X86_64),
        [
            "-products",
            "*",
            "-requires",
            "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
            "-latest",
            "-property",
            "installationPath",
            "-nologo",
        ]
    );
}

#[test]
fn arm64_vswhere_query_requires_arm64_tools_component() {
    let args = msvc_vswhere_installation_path_args(WindowsTargetArch::Aarch64);
    assert_eq!(args[3], "Microsoft.VisualStudio.Component.VC.Tools.ARM64");
}

#[test]
fn arm64_xwin_sysroot_selects_only_arm64_libraries() {
    let temp = tempfile::tempdir().expect("tempdir");
    for arch in ["x86_64", "aarch64"] {
        for component in ["crt/lib", "sdk/lib/um", "sdk/lib/ucrt"] {
            std::fs::create_dir_all(temp.path().join(component).join(arch)).expect("sysroot dir");
        }
    }

    let paths = xwin_sysroot_lib_paths(temp.path(), WindowsTargetArch::Aarch64);
    assert_eq!(paths.len(), 3);
    assert!(paths.iter().all(|path| path.contains("aarch64")));
    assert!(paths.iter().all(|path| !path.contains("x86_64")));
}

#[test]
fn x64_only_xwin_sysroot_is_not_reused_for_arm64() {
    let temp = tempfile::tempdir().expect("tempdir");
    for component in ["crt/lib", "sdk/lib/um", "sdk/lib/ucrt"] {
        std::fs::create_dir_all(temp.path().join(component).join("x86_64")).expect("sysroot dir");
    }

    assert!(xwin_sysroot_lib_paths(temp.path(), WindowsTargetArch::Aarch64).is_empty());
}

#[cfg(target_arch = "x86_64")]
#[test]
fn arm64_target_never_falls_back_to_bare_x64_artifact_dirs() {
    let candidates = collect_library_candidates("perry_stdlib.lib", Some("windows-aarch64"));
    assert!(!candidates
        .iter()
        .any(|path| path == std::path::Path::new("target/release/perry_stdlib.lib")));
    assert!(!candidates
        .iter()
        .any(|path| path == std::path::Path::new("target/debug/perry_stdlib.lib")));
}
