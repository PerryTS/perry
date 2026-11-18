//! Android + perry/ui: keep perry-stdlib's bundled `jni` crate out of
//! perry-ui-android's way.
//!
//! Both archives on an Android UI link line bundle their own copy of the `jni`
//! crate: perry-ui-android uses it for every JNI call, and perry-stdlib gets
//! one through perry-runtime → hickory-proto (which only names
//! `jni::errors::Error` in an enum variant, but a staticlib carries the whole
//! non-generic object code of every dependency). The stdlib is linked first
//! with `--allow-multiple-definition`, so for every jni function both copies
//! export — `AttachGuard::from_unowned`, `AttachGuard::borrow_env_mut`, the
//! `Drop` impls, … — the stdlib's definition wins. ThinLTO renamed the stdlib
//! copy's `THREAD_GUARD_NEST_LEVEL` thread-local (a `.llvm.<hash>` suffix) but
//! not its accessors, so the winning `from_unowned` bumps the stdlib's guard
//! level while perry-ui-android's own `JavaVM::thread_attach_guard_level` (and
//! every inlined `Env::assert_top`) reads the UI copy. The first JNI call that
//! creates a local reference — `find_class` in `PerryBridge.nativeInit` — then
//! aborts at launch:
//!
//! ```text
//! jni runtime check failure: attempt to create a new local reference using
//! an Env<'not_top> that is not associated with the top JNI stack frame.
//! ```
//!
//! Localizing the stdlib copy's jni/jni-sys globals makes every jni reference
//! from perry-ui-android bind to perry-ui-android's own, self-consistent copy;
//! the stdlib's copy stays reachable only from inside its own members (where
//! nothing on the Android path calls it).

use anyhow::Result;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::{collect_archive_global_symbols_flat, find_path_tool, strip_tmp_base};

/// True for a Rust v0-mangled symbol that names an item of the `jni` or
/// `jni_sys` crate anywhere in its path (including generic instantiations
/// such as `Arc<jni::errors::Error>::drop_slow`). A crate root is encoded as
/// `C` + optional disambiguator `s<base62>_` + `<len><ident>`, so the ident is
/// always preceded by `C` or the disambiguator's closing `_`, which rules out
/// longer identifiers whose length prefix merely ends in `3`/`7`.
pub(crate) fn is_jni_crate_symbol(sym: &str) -> bool {
    if !sym.starts_with("_R") {
        return false;
    }
    ["3jni", "7jni_sys"].iter().any(|ident| {
        sym.match_indices(ident).any(|(at, _)| {
            let prev = sym[..at].chars().next_back();
            let next = sym[at + ident.len()..].chars().next();
            // `3jni` must not be the head of `7jni_sys` or a longer ident run.
            matches!(prev, Some('C') | Some('_'))
                && next.map_or(true, |c| !c.is_ascii_lowercase() && c != '_')
        })
    })
}

/// Return a copy of `stdlib_lib` whose `jni`/`jni_sys` globals are local, or
/// `stdlib_lib` itself when it has none or a tool is missing (best-effort, like
/// the other strip-dedup passes: failure keeps the pre-fix link line).
/// `tool_dir` is the NDK's LLVM `bin/` (preferred: it matches the archives).
pub(crate) fn localize_bundled_jni_in_stdlib(
    stdlib_lib: &Path,
    tool_dir: Option<&Path>,
) -> PathBuf {
    match try_localize(stdlib_lib, tool_dir) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[strip-dedup] android stdlib jni localize skipped (non-fatal): {e}");
            stdlib_lib.to_path_buf()
        }
    }
}

fn tool(tool_dir: Option<&Path>, name: &str) -> Option<PathBuf> {
    let exe = if cfg!(target_os = "windows") {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    tool_dir
        .map(|d| d.join(&exe))
        .filter(|p| p.exists())
        .or_else(|| super::find_llvm_tool(name))
        .or_else(|| find_path_tool(name))
}

fn try_localize(stdlib_lib: &Path, tool_dir: Option<&Path>) -> Result<PathBuf> {
    let nm = tool(tool_dir, "llvm-nm").ok_or_else(|| anyhow::anyhow!("llvm-nm not found"))?;
    let objcopy =
        tool(tool_dir, "llvm-objcopy").ok_or_else(|| anyhow::anyhow!("llvm-objcopy not found"))?;
    let abs = std::fs::canonicalize(stdlib_lib)?;
    let mut syms: Vec<String> = collect_archive_global_symbols_flat(&nm, &abs)
        .into_iter()
        .filter(|s| is_jni_crate_symbol(s))
        .collect();
    if syms.is_empty() {
        return Ok(stdlib_lib.to_path_buf());
    }
    syms.sort();
    let lib_name = abs
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("libperry_stdlib.a");
    let tmp = strip_tmp_base();
    std::fs::create_dir_all(&tmp)?;
    let list = tmp.join(format!("_{lib_name}_jni_localize.txt"));
    let out_lib = tmp.join(format!("_{lib_name}_jni_localized.a"));
    std::fs::write(&list, syms.join("\n"))?;
    let _ = std::fs::remove_file(&out_lib);
    let out = Command::new(&objcopy)
        .arg(format!("--localize-symbols={}", list.display()))
        .arg(&abs)
        .arg(&out_lib)
        .output()?;
    let _ = std::fs::remove_file(&list);
    if !out.status.success() {
        return Err(anyhow::anyhow!(
            "llvm-objcopy failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    eprintln!(
        "[strip-dedup] {lib_name}: localized {} bundled jni global(s) so perry-ui-android's own jni copy is used",
        syms.len()
    );
    Ok(out_lib)
}

#[cfg(test)]
mod tests {
    use super::is_jni_crate_symbol;

    #[test]
    fn matches_jni_and_jni_sys_items() {
        // <jni::vm::java_vm::AttachGuard>::from_unowned
        assert!(is_jni_crate_symbol(
            "_RNvMs2_NtNtCs5LiBL7aUDKI_3jni2vm7java_vmNtB5_11AttachGuard12from_unowned"
        ));
        // Arc<jni::errors::Error>::drop_slow, instantiated in hickory_proto
        assert!(is_jni_crate_symbol(
            "_RNvMsn_NtNtCs9ovSSykGhWv_5alloc3rcs3arcINtB5_3ArcNtNtCs5LiBL7aUDKI_3jni6errors5ErrorE9drop_slowCshM7QNx9ysVn_13hickory_proto"
        ));
        // jni_sys item, crate root without disambiguator
        assert!(is_jni_crate_symbol("_RNvC7jni_sys3foo"));
    }

    #[test]
    fn ignores_other_crates() {
        assert!(!is_jni_crate_symbol("js_string_from_bytes"));
        assert!(!is_jni_crate_symbol("_ZN3jni3foo17h0123456789abcdefE")); // legacy mangling
                                                                          // perry_ui_android's own items are not jni-crate items
        assert!(!is_jni_crate_symbol(
            "_RNvNtCs1AbCdEfGhIj_16perry_ui_android10jni_bridge10find_class"
        ));
        // `13jniextensions` (a 13-char ident) contains `3jni` after a digit
        assert!(!is_jni_crate_symbol(
            "_RNvCs1AbCdEfGhIj_13jniextensions3foo"
        ));
    }
}
