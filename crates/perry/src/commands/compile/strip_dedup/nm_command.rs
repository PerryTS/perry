//! Native-symbol-table `nm` invocations for the strip/dedup pipeline.
//! Split out of `strip_dedup.rs` to keep the parent under the 2000-line gate.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Start an `llvm-nm` invocation that reads the object's NATIVE symbol table.
///
/// On a Mach-O member that still carries an `__LLVM,__bitcode` section
/// (unstripped local archives built with embedded bitcode), `llvm-nm` reads the
/// bitcode module's symbols instead of the nlist unless told not to. The two
/// can differ (internalized or merged symbols), so dedup would compare the
/// wrong sets and keep duplicate std/core members. `--no-llvm-bc` (LLVM >= 13)
/// pins the native view. A GNU binutils `nm` (the `PATH` fallback on Linux)
/// rejects the flag, and it never reads bitcode that way, so the flag is
/// added only for LLVM-based tools (`llvm-nm`, Apple's `nm`).
pub(super) fn nm_command(nm: &Path) -> Command {
    let mut cmd = Command::new(nm);
    if nm_is_llvm_based(nm) {
        cmd.arg("--no-llvm-bc");
    }
    cmd
}

fn nm_is_llvm_based(nm: &Path) -> bool {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<PathBuf, bool>>> =
        std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(known) = cache.lock().ok().and_then(|c| c.get(nm).copied()) {
        return known;
    }
    let llvm_based = Command::new(nm)
        .arg("--version")
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains("LLVM"));
    if let Ok(mut c) = cache.lock() {
        c.insert(nm.to_path_buf(), llvm_based);
    }
    llvm_based
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::super::{
        collect_archive_symbols_by_member, find_llvm_tool, find_nightly_llvm_tool, find_path_tool,
    };
    use std::path::Path;
    use std::process::Command;

    fn run_ok(cmd: &mut Command) {
        let out = cmd.output().expect("spawn fixture tool");
        assert!(
            out.status.success(),
            "{:?} failed: {}",
            cmd,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// A Mach-O member whose native nlist defines `native_only_sym` and whose
    /// embedded `__LLVM,__bitcode` module defines only `bitcode_only_sym`,
    /// archived as `lib.a`. Without `--no-llvm-bc`, `llvm-nm` reports the
    /// bitcode module's symbols.
    fn archive_with_diverging_bitcode(dir: &Path) -> std::path::PathBuf {
        let arch = if cfg!(target_arch = "aarch64") {
            "arm64"
        } else {
            "x86_64"
        };
        std::fs::write(dir.join("n.c"), "int native_only_sym(void){return 1;}\n").unwrap();
        std::fs::write(dir.join("b.c"), "int bitcode_only_sym(void){return 2;}\n").unwrap();
        run_ok(
            Command::new("clang")
                .current_dir(dir)
                .args(["-arch", arch, "-c", "n.c", "-o", "n.o"]),
        );
        run_ok(Command::new("clang").current_dir(dir).args([
            "-arch",
            arch,
            "-c",
            "-emit-llvm",
            "b.c",
            "-o",
            "b.bc",
        ]));
        run_ok(Command::new("ld").current_dir(dir).args([
            "-r",
            "-arch",
            arch,
            "n.o",
            "-sectcreate",
            "__LLVM",
            "__bitcode",
            "b.bc",
            "-o",
            "m.o",
        ]));
        run_ok(
            Command::new("ar")
                .current_dir(dir)
                .args(["rcs", "lib.a", "m.o"]),
        );
        dir.join("lib.a")
    }

    #[test]
    fn dedup_symbol_reader_returns_native_symbols_not_embedded_bitcode() {
        let temp = tempfile::tempdir().unwrap();
        let archive = archive_with_diverging_bitcode(temp.path());
        let nm = find_nightly_llvm_tool("llvm-nm")
            .or_else(|| find_llvm_tool("llvm-nm"))
            .or_else(|| find_path_tool("nm"))
            .expect("llvm-nm or Apple nm present");

        // Guard: the fixture really diverges. Plain `nm` (no flag) must see
        // the bitcode symbol, otherwise this test could not fail.
        let plain = Command::new(&nm)
            .args(["--defined-only", "--format=bsd"])
            .arg(&archive)
            .output()
            .unwrap();
        let plain = String::from_utf8_lossy(&plain.stdout);
        assert!(
            plain.contains("bitcode_only_sym") && !plain.contains("native_only_sym"),
            "fixture does not reproduce the bitcode view:\n{plain}"
        );

        let by_member = collect_archive_symbols_by_member(&nm, &archive).expect("nm succeeds");
        let symbols: std::collections::HashSet<&str> =
            by_member.values().flatten().map(String::as_str).collect();
        assert!(symbols.contains("_native_only_sym"), "{symbols:?}");
        assert!(!symbols.contains("_bitcode_only_sym"), "{symbols:?}");
    }
}
