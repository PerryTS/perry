//! `--target wasi` link (#11379): the program's wasm32 objects plus the
//! runtime built for `wasm32-wasip2`, linked by wasi-sdk into a WASI 0.2
//! component that runs under any WASI host (`wasmtime run app.wasm`).
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

/// Where wasi-sdk lives: `$WASI_SDK_PATH`, else its conventional prefix.
fn wasi_sdk() -> Result<PathBuf> {
    let root = std::env::var_os("WASI_SDK_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/opt/wasi-sdk"));
    if root.join("bin/clang").exists() && root.join("share/wasi-sysroot").is_dir() {
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

/// Link `objects` into the WASI component at `output`.
pub(crate) fn link_wasi(
    objects: &[PathBuf],
    runtime: &Path,
    output: &Path,
    verbose: u8,
) -> Result<()> {
    let sdk = wasi_sdk()?;
    let args = link_args(&sdk, objects, runtime, output);
    let clang = sdk.join("bin/clang");
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
             APIs beyond the core runtime (net, http, fetch, zlib, …) live in \
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
        assert_eq!(args[1], "--sysroot=/sdk/share/wasi-sysroot");
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
