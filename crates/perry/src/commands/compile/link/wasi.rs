//! `--target wasi` link (#11379): the program's wasm32 objects plus the
//! runtime built for `wasm32-wasip2`, linked by wasi-sdk into a WASI 0.2
//! component. The pinned SDK emits WASI 0.2.12 imports; the host must provide
//! those interfaces and Wasm EH (tested with Wasmtime 48).
//!
//! Deliberately separate from the native link path: no auto-optimize runtime
//! rebuild, no stdlib, no link cache. Only compiled with the off-by-default
//! `target-wasi` feature.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, bail, Context, Result};

/// Linear-memory stack size. Perry's shadow stack and deep recursion need
/// more than wasi-libc's 64 KiB default; placing the stack first also keeps
/// every heap address above the runtime's small-handle band.
const STACK_SIZE: u32 = 8 * 1024 * 1024;

fn sdk_clang(root: &Path) -> PathBuf {
    root.join("bin")
        .join(if cfg!(windows) { "clang.exe" } else { "clang" })
}

/// Where wasi-sdk lives: `$WASI_SDK_PATH`, else its conventional prefix.
fn wasi_sdk() -> Result<PathBuf> {
    let root = std::env::var_os("WASI_SDK_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/opt/wasi-sdk"));
    if sdk_clang(&root).is_file() && root.join("share/wasi-sysroot").is_dir() {
        return Ok(root);
    }
    bail!(
        "--target wasi links with wasi-sdk, which was not found at {} \
         (set WASI_SDK_PATH to its install directory; releases: \
         https://github.com/WebAssembly/wasi-sdk/releases)",
        root.display()
    )
}

/// The clang/wasm-ld arguments for one link, split out for testing.
pub(super) fn link_args(
    sdk: &Path,
    objects: &[PathBuf],
    runtime: &Path,
    output: &Path,
) -> Vec<String> {
    let mut args = vec![
        "--target=wasm32-wasip2".to_string(),
        format!("--sysroot={}", sdk.join("share/wasi-sysroot").display()),
    ];
    args.extend(objects.iter().map(|o| o.display().to_string()));
    args.push(runtime.display().to_string());
    args.push("-lsetjmp".to_string());
    args.extend([
        // The component adapter allocates through the module's allocator.
        "-Wl,--export=cabi_realloc".to_string(),
        "-Wl,--stack-first".to_string(),
        format!("-Wl,-z,stack-size={STACK_SIZE}"),
        // wasm-ld only warns on a call whose type differs from its callee's
        // and links a stub that traps when reached. That is always a
        // codegen/runtime ABI bug; refuse to produce such a module.
        "-Wl,--fatal-warnings".to_string(),
        "-o".to_string(),
        output.display().to_string(),
    ]);
    args
}

/// WASI uses the same feature selection and constructor registration as
/// native links. Omitting this object silently leaves optional runtime hooks
/// empty (for example Unicode normalization and URL prototype accessors).
pub(crate) fn feature_installer(
    ctx: &super::super::CompilationContext,
    cli_features: &[String],
    directory: &Path,
) -> Result<PathBuf> {
    use crate::commands::stdlib_installs::{runtime_installer_callees, FeatureInstalls};
    let selected = super::super::optimized_libs::auto_optimized_cross_features(
        ctx,
        &std::collections::BTreeSet::new(),
        cli_features,
    )
    .into_iter()
    .filter_map(|feature| feature.strip_prefix("perry-runtime/").map(str::to_string))
    .collect();
    let symbols = runtime_installer_callees(&FeatureInstalls::Selected(selected));
    let bytes =
        perry_codegen::stubs::generate_feature_installer_object(&symbols, None, &[], Some("wasi"))?;
    let path = directory.join("_perry_feature_installs.o");
    std::fs::write(&path, bytes)?;
    Ok(path)
}

/// Archive the reusable library objects for a separate canonical adapter link.
pub(crate) fn archive_library(
    objects: &[PathBuf],
    runtime: &Path,
    output: &Path,
    verbose: u8,
) -> Result<()> {
    let sdk = wasi_sdk()?;
    let archiver = sdk.join("bin").join(if cfg!(windows) {
        "llvm-ar.exe"
    } else {
        "llvm-ar"
    });
    // Use LLVM's Wasm-aware indexer, including on hosts whose system ar only
    // recognizes native objects. Recreate so removed modules cannot linger.
    if output.exists() {
        std::fs::remove_file(output)?;
    }
    let mut command = Command::new(&archiver);
    command.arg("crs").arg(output).args(objects);
    let status = super::super::tool_output::run_internal_tool(&mut command, verbose)?;
    if !status.success() {
        bail!("WASI static archive creation failed");
    }
    let manifest = serde_json::json!({
        "version": 1,
        "archive": output.canonicalize()?.display().to_string(),
        "entry_symbol": "perry_module_init",
        "target": "wasi",
        "link_archives": [{"role": "runtime", "path": runtime.canonicalize()?.display().to_string()}],
        "system_libraries": ["setjmp"]
    });
    std::fs::write(
        output.with_extension("linkdeps.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    Ok(())
}

/// Link `objects` into the WASI command component at `output`.
pub(crate) fn link_wasi(
    objects: &[PathBuf],
    runtime: &Path,
    output: &Path,
    verbose: u8,
) -> Result<()> {
    let sdk = wasi_sdk()?;
    let args = link_args(&sdk, objects, runtime, output);
    let clang = sdk_clang(&sdk);
    if verbose > 0 {
        eprintln!("  wasi link: {} {}", clang.display(), args.join(" "));
    }
    let out = Command::new(&clang)
        .args(&args)
        .output()
        .with_context(|| format!("running {}", clang.display()))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let hint = if stderr.contains("undefined symbol") {
            "\n\nA symbol the runtime does not provide on WASI yet: most Node \
             APIs beyond the core runtime (http, fetch, zlib, …) live in \
             perry-stdlib, which is not available for --target wasi (#11377)."
        } else {
            ""
        };
        return Err(anyhow!("wasi link failed:\n{}{hint}", stderr.trim_end()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_a_wasip2_component_and_refuses_signature_mismatches() {
        let args = link_args(
            Path::new("/sdk"),
            &[PathBuf::from("a.o"), PathBuf::from("b.o")],
            Path::new("/rt/libperry_runtime.a"),
            Path::new("app.wasm"),
        );
        assert_eq!(args[0], "--target=wasm32-wasip2");
        assert_eq!(
            args[1],
            format!(
                "--sysroot={}",
                Path::new("/sdk").join("share/wasi-sysroot").display()
            )
        );
        assert_eq!(&args[2..5], ["a.o", "b.o", "/rt/libperry_runtime.a"]);
        for flag in [
            "-Wl,--export=cabi_realloc",
            "-Wl,--stack-first",
            "-Wl,--fatal-warnings",
        ] {
            assert!(args.iter().any(|a| a == flag), "{flag} missing: {args:?}");
        }
        assert_eq!(args[args.len() - 2..], ["-o", "app.wasm"]);
    }
}
