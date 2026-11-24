//! Arguments a native host passes to its C compiler driver, beside the archives
//! of `<output>.linkdeps.json`, to link a `--output-type staticlib` archive.

use std::path::PathBuf;
use std::process::Command;

use perry_codegen::target_os_of;

use super::CompilationContext;

pub(in crate::commands::compile) fn staticlib_link_args(
    ctx: &CompilationContext,
    target: Option<&str>,
    well_known_libs: &[PathBuf],
) -> Vec<String> {
    let mut cmd = Command::new("cc");
    let os = target.map_or(Some(std::env::consts::OS), target_os_of);
    let apple = matches!(os, Some("macos" | "ios" | "tvos" | "watchos" | "visionos"));
    if apple {
        // Perry's own link drops unreached code, and with it references to
        // wrapper archives the program does not link.
        cmd.arg("-Wl,-dead_strip");
    }
    if os != Some("windows") {
        super::runtime_layouts::add_link_args(&mut cmd, apple, false, false);
        super::wrapper_hooks::add_link_args(&mut cmd, apple, false, false, well_known_libs);
    }
    if os == Some("macos") {
        super::macos_libs::add_system_libs(&mut cmd, ctx);
        if ctx.needs_ui {
            super::macos_libs::add_ui_frameworks(&mut cmd);
        }
    }
    cmd.get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(target: &str, needs_stdlib: bool, needs_ui: bool, libs: &[&str]) -> Vec<String> {
        let mut ctx = CompilationContext::new(PathBuf::from("."));
        ctx.needs_stdlib = needs_stdlib;
        ctx.needs_ui = needs_ui;
        let libs: Vec<PathBuf> = libs.iter().map(PathBuf::from).collect();
        staticlib_link_args(&ctx, Some(target), &libs)
    }

    fn has(args: &[String], arg: &str) -> bool {
        args.iter().any(|candidate| candidate == arg)
    }

    #[test]
    fn apple_hosts_admit_every_weak_runtime_reference() {
        for target in ["macos", "macos-x86_64", "ios", "tvos-simulator"] {
            let args = args(target, false, false, &[]);
            assert!(has(&args, "-Wl,-dead_strip"), "{target}");
            assert!(has(&args, "-Wl,-U,_js_ext_net_nm_install"), "{target}");
            assert!(!has(&args, "-Wl,-u,_js_ext_net_nm_install"), "{target}");
            for name in perry_abi::NATIVE_LAYOUT_DESCRIPTORS {
                assert_eq!(
                    has(&args, &format!("-Wl,-U,_perry_runtime_layout_{name}")),
                    !perry_abi::NATIVE_LAYOUT_RESERVED.contains(name),
                    "{target} {name}"
                );
            }
        }
    }

    #[test]
    fn a_listed_net_wrapper_is_forced_into_the_host_link() {
        let libs = ["/x/libperry_ext_net.a"];
        assert!(has(
            &args("macos", true, false, &libs),
            "-Wl,-u,_js_ext_net_nm_install"
        ));
        assert_eq!(
            args("linux", true, false, &libs),
            ["-Wl,-u,js_ext_net_nm_install"]
        );
        assert!(args("windows", true, false, &libs).is_empty());
    }

    #[test]
    fn macos_hosts_get_the_frameworks_perrys_own_link_uses() {
        let runtime_only = args("macos", false, false, &[]);
        assert!(!has(&runtime_only, "-framework"));
        let stdlib = args("macos", true, false, &[]);
        assert!(has(&stdlib, "Security") && has(&stdlib, "-lobjc"));
        assert_eq!(args("macos-x86_64", true, false, &[]), stdlib);
        assert!(!has(&stdlib, "AppKit"));
        assert!(has(&args("macos", false, true, &[]), "AppKit"));
        assert!(!has(&args("ios", true, true, &[]), "-framework"));
    }
}
