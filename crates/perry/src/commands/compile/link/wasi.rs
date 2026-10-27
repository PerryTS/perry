//! `--target wasi` link (#11379): the program's wasm32 objects plus the
//! runtime built for `wasm32-wasip2`, linked by wasi-sdk into a WASI 0.2
//! component. The pinned SDK emits WASI 0.2.12 imports; the host must provide
//! those interfaces and Wasm EH (tested with Wasmtime 48).
//!
//! `--target wasip1` is the same pipeline for a WASI preview1 core module
//! (runtime built for `wasm32-wasip1`): it imports only
//! `wasi_snapshot_preview1`, carries no component-model adapter and so has no
//! `cabi_realloc`, and is not wrapped into a component.
//!
//! Deliberately separate from the native link path: no auto-optimize runtime
//! rebuild, no stdlib, no link cache. Only compiled with the off-by-default
//! `target-wasi` feature.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, bail, Context, Result};

use super::super::wasi_flavour::Flavour;

/// Linear-memory stack size. Perry's shadow stack and deep recursion need
/// more than wasi-libc's 64 KiB default; placing the stack first also keeps
/// every heap address above the runtime's small-handle band.
const STACK_SIZE: u32 = 8 * 1024 * 1024;

fn sdk_clang(root: &Path) -> PathBuf {
    root.join("bin")
        .join(if cfg!(windows) { "clang.exe" } else { "clang" })
}

/// Where wasi-sdk lives: `$WASI_SDK_PATH`, else its conventional prefix.
fn wasi_sdk(flavour: Flavour) -> Result<PathBuf> {
    let root = std::env::var_os("WASI_SDK_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/opt/wasi-sdk"));
    if sdk_clang(&root).is_file() && root.join("share/wasi-sysroot").is_dir() {
        return Ok(root);
    }
    bail!(
        "--target {} links with wasi-sdk, which was not found at {} \
         (set WASI_SDK_PATH to its install directory; releases: \
         https://github.com/WebAssembly/wasi-sdk/releases)",
        flavour.target(),
        root.display()
    )
}

/// The clang/wasm-ld arguments for one link, split out for testing.
pub(super) fn link_args(
    flavour: Flavour,
    sdk: &Path,
    objects: &[PathBuf],
    runtime: &Path,
    output: &Path,
    keep_symbols: bool,
) -> Vec<String> {
    let mut args = vec![
        format!("--target={}", flavour.triple()),
        format!("--sysroot={}", sdk.join("share/wasi-sysroot").display()),
    ];
    args.extend(objects.iter().map(|o| o.display().to_string()));
    args.push(runtime.display().to_string());
    if !keep_symbols {
        // Compression changes instruction offsets and cannot preserve DWARF.
        args.extend([
            "-Wl,--strip-all".to_string(),
            "-Wl,--compress-relocations".to_string(),
        ]);
    }
    if flavour == Flavour::P2 {
        // The component adapter allocates through the module's allocator.
        args.push("-Wl,--export=cabi_realloc".to_string());
    }
    args.extend([
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
///
/// Both flavours link the core runtime, so only the runtime features this
/// program selects install.
pub(crate) fn feature_installer(
    flavour: Flavour,
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
    let bytes = perry_codegen::stubs::generate_feature_installer_object(
        &symbols,
        None,
        &[],
        Some(flavour.target()),
    )?;
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
    let sdk = wasi_sdk(Flavour::P2)?;
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
        "system_libraries": []
    });
    std::fs::write(
        output.with_extension("linkdeps.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    Ok(())
}

/// Link `objects` into the WASI command component (P2) or core module (P1) at
/// `output`.
pub(crate) fn link_wasi(
    flavour: Flavour,
    objects: &[PathBuf],
    runtime: &Path,
    output: &Path,
    verbose: u8,
) -> Result<()> {
    let sdk = wasi_sdk(flavour)?;
    // Match the native strip escape hatches. In particular, keep DWARF intact:
    // neither relocation compression nor wasm-opt may rewrite these offsets.
    let keep_symbols =
        std::env::var("PERRY_KEEP_SYMBOLS").is_ok() || std::env::var("PERRY_DEBUG_SYMBOLS").is_ok();
    let optimizer = if keep_symbols {
        None
    } else {
        let wasm_opt = find_tool(&sdk, "wasm-opt");
        let wasm_tools = find_tool(&sdk, "wasm-tools");
        match (wasm_opt, wasm_tools, flavour) {
            // A core module needs no component wrapping, so no wasm-tools.
            (Some(opt), _, Flavour::P1) => Some((opt, None)),
            (Some(opt), Some(component), Flavour::P2) => Some((opt, Some(component))),
            (opt, _, _) => {
                if verbose > 0 {
                    let missing = if opt.is_none() {
                        "wasm-opt"
                    } else {
                        "wasm-tools"
                    };
                    eprintln!(
                        "  wasi: skipping wasm-opt size optimization ({missing} not found in {} or PATH)",
                        sdk.join("bin").display()
                    );
                }
                None
            }
        }
    };
    // Each link owns its scratch directory, including concurrent links and
    // failures. A P2 output is always a component, even without tools.
    let scratch = optimizer
        .as_ref()
        .map(|_| {
            tempfile::Builder::new()
                .prefix("perry-wasi-link-")
                .tempdir()
        })
        .transpose()?;
    let core = scratch.as_ref().map(|d| d.path().join("core.wasm"));
    let mut args = link_args(
        flavour,
        &sdk,
        objects,
        runtime,
        core.as_deref().unwrap_or(output),
        keep_symbols,
    );
    if optimizer.is_some() && flavour == Flavour::P2 {
        args.insert(0, "-Wl,--skip-wit-component".to_string());
    }
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
            format!(
                "\n\nA symbol the runtime does not provide on WASI yet: most Node \
                 APIs beyond the core runtime (http, fetch, zlib, …) live in \
                 perry-stdlib, which is not available for --target {} (#11377).",
                flavour.target()
            )
        } else {
            String::new()
        };
        return Err(anyhow!("wasi link failed:\n{}{hint}", stderr.trim_end()));
    }
    if let Some((wasm_opt, wasm_tools)) = optimizer {
        let directory = scratch.as_ref().expect("optimizer scratch directory");
        // A core module is the final product; a component is built from it.
        let optimized = if wasm_tools.is_some() {
            directory.path().join("optimized.wasm")
        } else {
            output.to_path_buf()
        };
        let mut command = Command::new(&wasm_opt);
        command
            .arg(core.as_ref().expect("linked core module"))
            .args(WASM_OPT_ARGS)
            .arg("-o")
            .arg(&optimized);
        run_size_tool(&mut command, verbose, "wasm-opt")?;
        if let Some(wasm_tools) = wasm_tools {
            // Some WASIp2 runtime dependencies still import Preview 1 (for
            // example getrandom in TCP). Preserve wasm-component-ld's default
            // reactor adapter (the core exports the Preview 2 run entry, not
            // `_start`) instead of accepting only pure Preview 2 modules.
            let adapter = directory.path().join("wasi_snapshot_preview1.reactor.wasm");
            std::fs::write(
                &adapter,
                wasi_preview1_component_adapter_provider::WASI_SNAPSHOT_PREVIEW1_REACTOR_ADAPTER,
            )?;
            let mut command = Command::new(&wasm_tools);
            command
                .arg("component")
                .arg("new")
                .arg(&optimized)
                .arg("--adapt")
                .arg(format!("wasi_snapshot_preview1={}", adapter.display()))
                .arg("-o")
                .arg(output);
            run_size_tool(&mut command, verbose, "wasm-tools component new")?;
        }
    }
    Ok(())
}

// Only enable features used by wasi-sdk/Rust. `-all` also enables proposals
// that the supported Wasmtime host cannot load.
const WASM_OPT_ARGS: &[&str] = &[
    "-Oz",
    "--enable-exception-handling",
    "--enable-bulk-memory",
    "--enable-bulk-memory-opt",
    "--enable-nontrapping-float-to-int",
    "--enable-sign-ext",
    "--enable-mutable-globals",
    "--enable-multivalue",
    "--enable-reference-types",
    "--enable-call-indirect-overlong",
];

/// Prefer companion tools in the configured wasi-sdk; allow standalone tools
/// on PATH as well. Release packaging can install both beside SDK clang.
fn find_tool(sdk: &Path, name: &str) -> Option<PathBuf> {
    let name = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    };
    std::iter::once(sdk.join("bin"))
        .chain(
            std::env::var_os("PATH")
                .iter()
                .flat_map(std::env::split_paths),
        )
        .map(|directory| directory.join(&name))
        .find(|path| path.is_file())
}

fn run_size_tool(command: &mut Command, verbose: u8, stage: &str) -> Result<()> {
    if verbose > 0 {
        eprintln!("  wasi {stage}: {command:?}");
    }
    let status = super::super::tool_output::run_internal_tool(command, verbose)
        .with_context(|| format!("running {stage}"))?;
    if !status.success() {
        bail!("WASI {stage} failed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(flavour: Flavour, keep_symbols: bool) -> Vec<String> {
        link_args(
            flavour,
            Path::new("/sdk"),
            &[PathBuf::from("a.o"), PathBuf::from("b.o")],
            Path::new("/rt/libperry_runtime.a"),
            Path::new("app.wasm"),
            keep_symbols,
        )
    }

    fn sysroot() -> String {
        format!(
            "--sysroot={}",
            Path::new("/sdk").join("share/wasi-sysroot").display()
        )
    }

    #[test]
    fn links_a_wasip2_component_and_refuses_signature_mismatches() {
        let args = link(Flavour::P2, false);
        assert_eq!(args[0], "--target=wasm32-wasip2");
        assert_eq!(args[1], sysroot());
        assert_eq!(&args[2..5], ["a.o", "b.o", "/rt/libperry_runtime.a"]);
        for flag in [
            "-Wl,--export=cabi_realloc",
            "-Wl,--stack-first",
            "-Wl,--fatal-warnings",
            "-Wl,--strip-all",
            "-Wl,--compress-relocations",
        ] {
            assert!(args.iter().any(|a| a == flag), "{flag} missing: {args:?}");
        }
        assert_eq!(args[args.len() - 2..], ["-o", "app.wasm"]);
    }

    /// The whole P2 recipe, pinned: `--target wasi` must keep linking exactly
    /// the way it did before `--target wasip1` shared this code.
    #[test]
    fn wasip2_link_args_are_unchanged() {
        assert_eq!(
            link(Flavour::P2, false),
            [
                "--target=wasm32-wasip2",
                sysroot().as_str(),
                "a.o",
                "b.o",
                "/rt/libperry_runtime.a",
                "-lsetjmp",
                "-Wl,--strip-all",
                "-Wl,--compress-relocations",
                "-Wl,--export=cabi_realloc",
                "-Wl,--stack-first",
                "-Wl,-z,stack-size=8388608",
                "-Wl,--fatal-warnings",
                "-o",
                "app.wasm",
            ]
        );
    }

    /// P1 is the P2 recipe for the preview1 triple, minus the component
    /// allocator export: it is a core module, not a component.
    #[test]
    fn wasip1_links_a_core_module_without_the_component_allocator() {
        assert_eq!(
            link(Flavour::P1, false),
            [
                "--target=wasm32-wasip1",
                sysroot().as_str(),
                "a.o",
                "b.o",
                "/rt/libperry_runtime.a",
                "-lsetjmp",
                "-Wl,--strip-all",
                "-Wl,--compress-relocations",
                "-Wl,--stack-first",
                "-Wl,-z,stack-size=8388608",
                "-Wl,--fatal-warnings",
                "-o",
                "app.wasm",
            ]
        );
    }

    #[test]
    fn keeping_symbols_preserves_debug_offsets() {
        for flavour in [Flavour::P2, Flavour::P1] {
            let args = link(flavour, true);
            assert!(!args.iter().any(|arg| arg.contains("--strip")));
            assert!(!args
                .iter()
                .any(|arg| arg.contains("--compress-relocations")));
        }
    }

    #[test]
    fn flavours_map_to_their_target_names_and_triples() {
        assert_eq!(Flavour::from_target("wasi"), Some(Flavour::P2));
        assert_eq!(Flavour::from_target("wasip1"), Some(Flavour::P1));
        for other in ["wasip2", "wasm", "web", "linux", "macos", ""] {
            assert_eq!(Flavour::from_target(other), None, "{other}");
        }
        for flavour in [Flavour::P2, Flavour::P1] {
            assert_eq!(Flavour::from_target(flavour.target()), Some(flavour));
        }
        assert_eq!(Flavour::P2.triple(), "wasm32-wasip2");
        assert_eq!(Flavour::P1.triple(), "wasm32-wasip1");
    }
}
