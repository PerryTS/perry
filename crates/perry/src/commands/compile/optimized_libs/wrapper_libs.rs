use std::path::PathBuf;

use crate::OutputFormat;

use super::super::rust_target_triple;

/// #2532 / #3954 — the prebuilt `perry-ext-*` wrapper archives a program's
/// routed modules link (its `routed_modules`).
///
/// The wrappers are built with the runtime and stdlib archives, in one Cargo
/// graph, when perry is built (they are workspace default members); perry
/// never builds one while compiling a program. The prebuilt stdlib carries the
/// `external-*` adapters each wrapper needs, installed by the generated
/// installer, and the link places the wrappers before the stdlib so their
/// symbols satisfy the program's objects first.
///
/// Each wrapper is located through `find_library`, which honours the
/// `PERRY_LIB_DIR` / `PERRY_RUNTIME_DIR` overrides and the exe-dir /
/// Homebrew `../lib` probes. A missing wrapper is a broken install or an
/// incomplete workspace build, and stops the compile with what to build.
pub(crate) fn resolve_prebuilt_ext_libs(
    iteration_set: &std::collections::BTreeSet<String>,
    target: Option<&str>,
    format: OutputFormat,
) -> Vec<PathBuf> {
    let mut libs: Vec<PathBuf> = Vec::new();
    // Dedup by lib basename — http / https / http2 all map to
    // `perry_ext_http`, so without this the same `.a` would be added three
    // times.
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for module in iteration_set {
        let Some(binding) = super::super::well_known::lookup_well_known(module) else {
            continue;
        };
        if !seen.insert(binding.lib.clone()) {
            continue;
        }
        let filename =
            super::variant_archive_name(&super::super::well_known::ext_staticlib_filename(
                &binding.lib,
                rust_target_triple(target),
            ));
        match super::super::library_search::find_library(&filename, target) {
            Some(path) => {
                if matches!(format, OutputFormat::Text) {
                    println!(
                        "  well-known: routing `{}` → {} ({})",
                        module,
                        path.display(),
                        binding.tracking.as_deref().unwrap_or("no tracking issue")
                    );
                }
                libs.push(path);
            }
            None => {
                let npm_hint = super::super::library_search::npm_ext_package_install_hint()
                    .map(|cmd| {
                        format!(
                            " This perry runs from an npm package, which ships the wrapper \
                             archives separately: `{cmd}`."
                        )
                    })
                    .unwrap_or_default();
                eprintln!(
                    "error: `{module}` links the prebuilt wrapper archive `{filename}`, which \
                     was not found. Install Perry's bundled libraries next to the perry binary, \
                     set PERRY_LIB_DIR, or build the workspace (`cargo build --release` builds \
                     `{}` with the runtime; `scripts/build_instr_runtime.sh` builds the \
                     instrumented `_instr` archives a compile with an instrument knob links).{npm_hint}",
                    binding.krate
                );
                std::process::exit(1);
            }
        }
    }
    libs
}
