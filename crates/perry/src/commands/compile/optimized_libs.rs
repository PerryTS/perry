//! The runtime/stdlib/wrapper archives a program links (`OptimizedLibs`) and
//! the optional features its generated installer installs. perry links the
//! prebuilt archives and never builds them while compiling a program; the
//! driver (`build_optimized_libs`) only selects archives and installs.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::ffi::OsStr;
use std::path::PathBuf;

use super::CompilationContext;

mod driver;
mod features;
mod paths;
mod wrapper_libs;

pub(crate) use driver::build_optimized_libs;
pub(crate) use features::{
    auto_optimized_cross_features, gc_diagnostics_requested, gc_instruments_requested,
    gc_trace_requested, hot_diag_requested, instrumented_variant_requested, variant_archive_name,
};

pub(crate) use paths::android_global_dynamic_tls_rustflag;

#[cfg(test)]
mod tests;

pub struct OptimizedLibs {
    /// The prebuilt wrapper archives (#466 Phase 4), one per well-known native
    /// binding the program's routed modules link.
    pub well_known_libs: Vec<PathBuf>,
    /// Wrapper archives go before the stdlib, so their symbols satisfy the
    /// program's objects first (the stdlib also defines some of the Node
    /// binding symbols a wrapper replaces).
    pub prefer_well_known_before_stdlib: bool,
    /// Which stdlib feature installs the generated
    /// `perry_stdlib_feature_installer` calls (see `stdlib_installs.rs`).
    pub stdlib_installs: crate::commands::stdlib_installs::FeatureInstalls,
    /// Which runtime feature installs the generated installer calls; the
    /// runtime counterpart of `stdlib_installs`.
    pub runtime_installs: crate::commands::stdlib_installs::FeatureInstalls,
}

impl OptimizedLibs {
    pub(super) fn empty() -> Self {
        OptimizedLibs {
            well_known_libs: Vec::new(),
            prefer_well_known_before_stdlib: false,
            stdlib_installs: crate::commands::stdlib_installs::FeatureInstalls::Compiled,
            runtime_installs: crate::commands::stdlib_installs::FeatureInstalls::Compiled,
        }
    }
}

pub(crate) fn well_known_iteration_set(ctx: &CompilationContext) -> BTreeSet<String> {
    let mut iteration_set: BTreeSet<String> = ctx.native_module_imports.iter().cloned().collect();
    if let Ok(forced) = std::env::var("PERRY_FORCE_WELL_KNOWN") {
        for module in forced.split(|ch: char| ch == ',' || ch == ';' || ch.is_whitespace()) {
            let module = module.trim();
            if module.is_empty() {
                continue;
            }
            if super::well_known::lookup_well_known(module).is_some() {
                iteration_set.insert(module.strip_prefix("node:").unwrap_or(module).to_string());
            }
        }
    }
    // tokio lane L4: `node:tls` is served by perry-stdlib's TLS module over
    // perry-ext-net's sockets — `tls.connect` and every client TLSSocket
    // method are ext-net's — and bundled `net`, which used to fill that role
    // for a TLS-only program, is gone. So a `tls` import routes `net` too.
    if iteration_set
        .iter()
        .any(|m| m.strip_prefix("node:").unwrap_or(m) == "tls")
    {
        iteration_set.insert("net".to_string());
    }
    iteration_set
}

/// Whether the well-known flip is on — i.e. `PERRY_DISABLE_WELL_KNOWN` unset.
pub(crate) fn well_known_flip_enabled() -> bool {
    std::env::var_os("PERRY_DISABLE_WELL_KNOWN").is_none()
}

/// Bindings whose wrapper crate is the ONLY implementation, so the flip routes
/// them even when PERRY_DISABLE_WELL_KNOWN=1 — disabling it reverts to
/// perry-stdlib's copies, and these have none. perry-stdlib's bundled `net`
/// (the other `js_net_socket_*` / `js_tls_connect`) and `ws` copies ran on
/// tokio sockets and were strict subsets of perry-ext-net / perry-ext-ws;
/// tokio lane L4 deleted them. perry-stdlib's `zlib.rs` is gone too (#12143):
/// its compression features link perry-ext-zlib itself. A `tls` import is
/// covered through `net`: `tls.connect` is perry-ext-net's, and its symbols
/// route to `net` (`perry_codegen::ext_registry`).
pub(crate) fn wrapper_is_sole_provider(module: &str) -> bool {
    matches!(
        module.strip_prefix("node:").unwrap_or(module),
        "net" | "ws" | "zlib"
    )
}

/// THE routing decision: which library serves each well-known native module
/// in this compile. Every binding in `well_known_bindings.toml` routes to its
/// wrapper crate normally; with PERRY_DISABLE_WELL_KNOWN=1 only the
/// [`wrapper_is_sole_provider`] ones do, and the rest are served by the
/// bundled runtime / stdlib. Computed once per compile
/// (`CompilationContext::native_routing`); codegen reads it for the install
/// symbols and wrapper calls it emits, and the linker for the stdlib features
/// and wrapper archives it links.
pub(crate) fn native_routing() -> perry_codegen::NativeRouting {
    let flip = well_known_flip_enabled();
    perry_codegen::NativeRouting::new(super::well_known::iter_well_known().map(|binding| {
        let provider = if flip || wrapper_is_sole_provider(&binding.package) {
            perry_codegen::NativeProvider::Wrapper(binding.lib.clone())
        } else {
            perry_codegen::NativeProvider::Bundled(binding.lib.clone())
        };
        (binding.package.clone(), provider)
    }))
}

/// The program's native modules ([`well_known_iteration_set`]) that the
/// routing decision serves from a wrapper archive.
pub(crate) fn routed_modules(ctx: &CompilationContext) -> BTreeSet<String> {
    ctx.native_routing.routed(&well_known_iteration_set(ctx))
}

/// Refuse a program that imports a native module the routing decision leaves
/// without a provider ([`perry_codegen::NativeRouting::unprovided`]): its
/// wrapper archive is not linked, and the bundled runtime does not implement
/// it, so its calls would compile and then fail at run time. The program's
/// modules are the same [`well_known_iteration_set`] the linker routes.
pub(crate) fn check_native_providers(ctx: &CompilationContext) -> anyhow::Result<()> {
    let errors: Vec<String> = well_known_iteration_set(ctx)
        .iter()
        .filter(|module| ctx.native_routing.unprovided(module))
        .map(|module| {
            let binding = super::well_known::lookup_well_known(module);
            let name = match binding {
                Some(binding) if binding.node_builtin => format!("node:{module}"),
                _ => module.clone(),
            };
            let krate = binding.map_or("its wrapper crate", |binding| binding.krate.as_str());
            format!(
                "{name} has no provider: PERRY_DISABLE_WELL_KNOWN=1 disables its wrapper \
                 crate {krate}, and the bundled runtime doesn't implement it (the whole \
                 module is refused under this setting). Unset PERRY_DISABLE_WELL_KNOWN \
                 to link {krate}."
            )
        })
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(anyhow::anyhow!("{}", errors.join("\n")))
    }
}

/// Name wrapper archives needed by emitted object-file symbols but absent from
/// the link line. The caller has already scanned the runtime and stdlib too,
/// so a symbol those archives define does not produce a false missing-wrapper
/// diagnostic. Return one deterministic message per missing archive.
pub(crate) fn missing_ext_archive_diagnostics(
    emitted_undefined: &BTreeSet<String>,
    linked_definitions: &HashSet<String>,
    resolved_ext_libs: &[PathBuf],
    target: Option<&str>,
) -> Vec<String> {
    let mut missing: BTreeMap<String, (&str, &str)> = BTreeMap::new();
    for symbol in emitted_undefined {
        if linked_definitions.contains(symbol) {
            continue;
        }
        let Some(owner) = perry_codegen::ext_registry::well_known_owner_for_symbol(symbol) else {
            continue;
        };
        let Some(binding) = super::well_known::lookup_well_known(owner) else {
            continue;
        };
        let filename = variant_archive_name(&super::well_known::ext_staticlib_filename(
            &binding.lib,
            super::rust_target_triple(target),
        ));
        if resolved_ext_libs
            .iter()
            .any(|path| path.file_name() == Some(OsStr::new(&filename)))
        {
            continue;
        }
        missing.entry(filename).or_insert((symbol, &binding.krate));
    }
    missing
        .into_iter()
        .map(|(filename, (symbol, krate))| {
            format!(
                "`{symbol}` needs {filename}, but that archive is not linked. \
                 From a Perry source checkout, build it with: cargo build \
                 --release -p {krate}. For an installed Perry, install the \
                 matching extension archive in PERRY_RUNTIME_DIR or PERRY_LIB_DIR."
            )
        })
        .collect()
}
