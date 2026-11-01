//! P3's libcall thread-context ABI is newer than Perry's linked LLVM 22.
//! Optimize the explicit wasm32 shadow-root IR in process, then emit through
//! the same SDK that links the component. Native RS4GC never takes this path.
use anyhow::{anyhow, bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

pub fn clang_path() -> PathBuf {
    std::env::var_os("WASI_SDK_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/opt/wasi-sdk"))
        .join("bin")
        .join(if cfg!(windows) { "clang.exe" } else { "clang" })
}

fn validate(clang: &Path) -> Result<()> {
    let output = Command::new(clang)
        .arg("--version")
        .output()
        .with_context(|| {
            format!(
                "--target wasip3 requires wasi-sdk 34 or newer; cannot run {} (set WASI_SDK_PATH)",
                clang.display()
            )
        })?;
    let version = super::parse_version(&String::from_utf8_lossy(&output.stdout));
    if !output.status.success() || version.is_none_or(|v| v < (23, 0, 0)) {
        bail!(
            "--target wasip3 requires SDK Clang >= 23 for the libcall thread-context ABI; found {}",
            clang.display()
        );
    }
    let macros = Command::new(clang)
        .args(["--target=wasm32-wasip3", "-dM", "-E", "-x", "c", "-"])
        .stdin(Stdio::null())
        .output()?;
    if !macros.status.success()
        || !String::from_utf8_lossy(&macros.stdout)
            .lines()
            .any(|line| line == "#define __wasm_libcall_thread_context__ 1")
    {
        bail!(
            "{} does not provide the WASIp3 libcall thread-context ABI",
            clang.display()
        );
    }
    Ok(())
}

pub fn check() -> Result<()> {
    static CHECK: OnceLock<Result<(), String>> = OnceLock::new();
    CHECK
        .get_or_init(|| validate(&clang_path()).map_err(|e| format!("{e:#}")))
        .as_ref()
        .map(|_| ())
        .map_err(|error| anyhow!(error.clone()))
}

/// Include the resolved executable identity in P3 object-cache keys. A new
/// SDK at the same install path must not reuse objects from the previous ABI.
pub fn cache_key() -> &'static str {
    static KEY: OnceLock<String> = OnceLock::new();
    KEY.get_or_init(|| {
        let path = clang_path();
        let path = path.canonicalize().unwrap_or(path);
        let metadata = path.metadata().ok();
        let modified = metadata
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos());
        let version = Command::new(&path)
            .arg("--version")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default();
        format!(
            "p3-sdk-emitter-v1:{}:{:?}:{modified:?}:{version}",
            path.display(),
            metadata.map(|m| m.len())
        )
    })
}

pub(crate) fn emit(module: &inkwell::module::Module<'_>, assembly: bool) -> Result<Vec<u8>> {
    check()?;
    let scratch = tempfile::Builder::new()
        .prefix("perry-p3-emit-")
        .tempdir()?;
    let input = scratch.path().join("module.ll");
    let output = scratch
        .path()
        .join(if assembly { "module.s" } else { "module.o" });
    module
        .print_to_file(&input)
        .map_err(|error| anyhow!("P3 IR emission failed: {error}"))?;
    let clang = clang_path();
    let result = Command::new(&clang)
        .args([
            "--target=wasm32-wasip3",
            "-x",
            "ir",
            if assembly { "-S" } else { "-c" },
            // The in-process optimizer has already run. Never rerun native
            // statepoint rewriting or a second IR optimisation pipeline.
            "-O0",
            "-mexception-handling",
            "-mllvm",
            "-wasm-enable-eh",
            "-mllvm",
            "-wasm-use-legacy-eh=false",
        ])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .output()?;
    if !result.status.success() {
        let retained = scratch.keep();
        bail!(
            "P3 SDK object emission failed using {}; input retained at {}:\n{}",
            clang.display(),
            retained.join("module.ll").display(),
            String::from_utf8_lossy(&result.stderr)
        );
    }
    std::fs::read(output).context("reading SDK-emitted P3 object")
}
