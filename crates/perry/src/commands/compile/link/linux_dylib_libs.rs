//! System libraries for a Linux shared-library (`--output-type dylib`) link.
//!
//! The executable link (`build_and_run.rs`) has always added
//! `-lm -lpthread -ldl` on Linux. The `cc -shared` plugin link in
//! `run_pipeline.rs` never did: a plugin resolves every `perry_*`/`js_*`
//! symbol from the host at `dlopen` time, so nothing *Perry* provides is
//! linked in, and the omission was invisible as long as the objects only
//! referenced host symbols. LLVM lowers `llvm.floor`/`llvm.log10`/… in
//! generated closures to libm calls, though, and glibc's `floor`/`log10`
//! live in `libm.so`, so the first real-app ELF dylib link (a Next.js route
//! bundle, #8942) failed with `undefined reference to 'floor'` right after
//! the string-constant collisions were fixed. macOS never sees this because
//! `-lSystem` (implicit in `cc -dynamiclib`) carries libm.
//!
//! Kept next to `windows_link::add_system_libs` / `linux_ui_libs` rather than
//! inline in the 7k-line pipeline so the command shape is unit-testable.

use std::path::Path;
use std::process::Command;

/// The libraries, in link order. Same set the Linux executable link uses
/// (glibc ≥ 2.34 folds pthread/dl into libc, older glibc and the arm64
/// cross sysroots do not — the flags are harmless where redundant).
pub(crate) const LINUX_DYLIB_SYSTEM_LIBS: &[&str] = &["-lm", "-lpthread", "-ldl"];

/// Finish a Unix plugin link (`cc -shared` on Linux, `cc -dynamiclib` on
/// macOS) after the caller has pushed the object files: on Linux append the
/// system libraries, then the output path.
///
/// Order is load-bearing on GNU ld: `-l` flags resolve only references seen
/// *before* them on the command line (and Ubuntu's default `--as-needed`
/// drops a library nothing preceding it needs), so the libraries must
/// follow the objects. They therefore belong here, not in the per-platform
/// command prologue.
pub(crate) fn push_unix_dylib_output(cmd: &mut Command, is_linux: bool, exe_path: &Path) {
    if is_linux {
        for lib in LINUX_DYLIB_SYSTEM_LIBS {
            cmd.arg(lib);
        }
    }
    cmd.arg("-o").arg(exe_path);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(cmd: &Command) -> Vec<String> {
        cmd.get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn linux_shared_library_link_carries_libm_after_the_objects() {
        // #8942: `undefined reference to 'floor'` / `'log10'` from
        // `perry_closure_*` in a `--output-type dylib` link on Ubuntu.
        let mut cmd = Command::new("cc");
        cmd.arg("-shared").arg("route.o").arg("app-page.o");
        push_unix_dylib_output(&mut cmd, true, Path::new("out.so"));
        let args = args(&cmd);
        let pos = |flag: &str| {
            args.iter()
                .position(|a| a == flag)
                .unwrap_or_else(|| panic!("`{flag}` missing from {args:?}"))
        };
        let last_object = pos("app-page.o");
        for lib in LINUX_DYLIB_SYSTEM_LIBS {
            assert!(
                pos(lib) > last_object,
                "`{lib}` must follow the objects or GNU ld ignores it: {args:?}"
            );
            assert!(pos(lib) < pos("-o"), "`{lib}` must precede `-o`: {args:?}");
        }
        assert_eq!(&args[args.len() - 2..], ["-o", "out.so"]);
    }

    #[test]
    fn macos_shared_library_link_adds_no_system_libs() {
        // libSystem (implicit in `-dynamiclib`) already provides libm; the
        // Linux flags would only add noise to the ld64 command line.
        let mut cmd = Command::new("cc");
        cmd.arg("-dynamiclib").arg("plugin.o");
        push_unix_dylib_output(&mut cmd, false, Path::new("out.dylib"));
        let args = args(&cmd);
        assert!(!args
            .iter()
            .any(|a| LINUX_DYLIB_SYSTEM_LIBS.contains(&a.as_str())));
        assert_eq!(args, ["-dynamiclib", "plugin.o", "-o", "out.dylib"]);
    }
}
