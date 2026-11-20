//! Wrapper install hooks the runtime calls lazily.
//!
//! A program links a wrapper archive whose install hook its entry prologue
//! need not call: a `child_process` program links perry-ext-net because a
//! child's stdio pipes are `net.Socket`s, and perry-runtime runs
//! `js_ext_net_nm_install` (`node:net` alone) at the first pipe socket,
//! through a weak reference. A weak reference pulls no archive member, so that
//! hook is named as an undefined symbol on the link line. It is the only one:
//! an undefined symbol is also a `--gc-sections` root, and every other
//! wrapper hook is the entry prologue's to name, for the modules the program
//! imports. Darwin also admits the runtime's weak
//! reference by name (`-U`) for programs that link no perry-ext-net; ELF and
//! COFF resolve an undefined weak symbol to null by themselves.

use std::path::PathBuf;
use std::process::Command;

/// The install hook perry-runtime calls lazily (child_process pipe sockets),
/// and the wrapper archive that defines it.
const LAZY_RUNTIME_HOOK: &str = "js_ext_net_nm_install";
const LAZY_RUNTIME_HOOK_LIB: &str = "perry_ext_net";

pub(super) fn add_link_args(
    cmd: &mut Command,
    darwin: bool,
    direct_apple: bool,
    xlinker_apple: bool,
    well_known_libs: &[PathBuf],
) {
    let prefix = if darwin { "_" } else { "" };
    let mut arg = |flag: &str, symbol: &str| {
        let symbol = format!("{prefix}{symbol}");
        if direct_apple {
            cmd.arg(flag).arg(symbol);
        } else if xlinker_apple {
            cmd.arg("-Xlinker").arg(flag).arg("-Xlinker").arg(symbol);
        } else {
            cmd.arg(format!("-Wl,{flag},{symbol}"));
        }
    };
    let links_lazy_hook = well_known_libs.iter().any(|lib| {
        lib.file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| stem.trim_start_matches("lib") == LAZY_RUNTIME_HOOK_LIB)
    });
    if links_lazy_hook {
        arg("-u", LAZY_RUNTIME_HOOK);
    }
    if darwin {
        arg("-U", LAZY_RUNTIME_HOOK);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(darwin: bool, direct: bool, libs: &[&str]) -> Vec<String> {
        let mut cmd = Command::new("linker");
        let libs: Vec<PathBuf> = libs.iter().map(PathBuf::from).collect();
        add_link_args(&mut cmd, darwin, direct, false, &libs);
        cmd.get_args()
            .map(|arg| arg.to_str().unwrap().to_owned())
            .collect()
    }

    #[test]
    fn a_linked_wrapper_hook_is_forced_into_the_link() {
        assert_eq!(
            arguments(false, false, &["/x/libperry_ext_net.a"]),
            vec!["-Wl,-u,js_ext_net_nm_install"]
        );
        assert_eq!(
            arguments(true, true, &["/x/libperry_ext_net.a"]),
            vec![
                "-u",
                "_js_ext_net_nm_install",
                "-U",
                "_js_ext_net_nm_install"
            ]
        );
        // Another wrapper's module hook is the entry prologue's to name.
        assert!(arguments(false, false, &["/x/libperry_ext_ws.a"]).is_empty());
    }

    #[test]
    fn darwin_admits_the_lazy_hook_when_no_wrapper_links() {
        assert_eq!(
            arguments(true, false, &[]),
            vec!["-Wl,-U,_js_ext_net_nm_install"]
        );
        assert!(arguments(false, false, &[]).is_empty());
    }
}
