//! The runtime call-effect table a compilation reads (GC effects design §4.3):
//! the `.gc-effects` sidecar shipped beside the prebuilt runtime archive this
//! program links, checked against the archive bytes.
//!
//! The sidecar was computed from the archive bytes when the archives were
//! built (`scripts/gc_call_effects/sidecars.sh`); perry only reads it. Its key
//! is the size and SHA-256 of every archive of the link combination, re-hashed
//! here on every compile: a stamp or an mtime would be a cache, and a cache can
//! be stale. The table is the one of the archive that links, for its target
//! and variant (default or instrumented): nothing else is consulted.
//!
//! A sidecar that is missing, unreadable or keyed to other bytes is a hard
//! error in a perry that runs from its workspace (the fix is one command). An
//! installed perry compiles under the all-unsafe table instead and says so
//! once: sound, slower, and only reachable by replacing shipped files. A
//! compilation with no prebuilt runtime archive to describe (WASI, a
//! `--no-link` compile with no archive, Geisterhand's cargo-built runtime, the
//! runtime copy inside an Android/watchOS/visionOS UI library) compiles under
//! the all-unsafe table: no table speaks for what links.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Result};
use perry_codegen::GcEffects;
use perry_gc_effects::sidecar::{
    archive_key, combination_name, sidecar_path, ArchiveKey, Mismatch, Sidecar,
};

use super::library_search::{locate_runtime_library, locate_stdlib_library, LocatedLibrary};

/// What a compilation links, as far as the effects table is concerned.
pub(super) enum EffectsLinkSet {
    /// A runtime no sidecar describes: no native link set (a standalone WASI
    /// build), Geisterhand's runtime (cargo builds it after codegen, with the
    /// `geisterhand` feature), or the runtime copy bundled in the UI library
    /// on Android/watchOS/visionOS (`link::ui_lib_provides_runtime`). The
    /// table proves nothing.
    Undescribed(&'static str),
    /// The prebuilt runtime archive (variant chosen before codegen), and the
    /// stdlib archive when the link line carries it.
    Prebuilt { with_stdlib: bool },
}

/// Whether the final link puts the stdlib archive on the link line, given that
/// one is found. This is the condition `link::build_and_run` links under:
/// `ctx.needs_stdlib || is_windows` (Windows links the stdlib, which bundles
/// the runtime, unconditionally), plus the Linux GTK4 UI program that
/// force-links the stdlib for its glib trampolines.
pub(super) fn stdlib_on_link_line(
    needs_stdlib: bool,
    needs_ui: bool,
    target: Option<&str>,
) -> bool {
    let is_linux = matches!(target, Some(t) if t.starts_with("linux"))
        || (target.is_none() && cfg!(target_os = "linux"));
    needs_stdlib
        || super::is_windows_target(target)
        || (is_linux && needs_ui && super::find_ui_library(target).is_some())
}

/// The effects table for this compilation. `dev` is whether perry runs from
/// its workspace (`find_perry_workspace_root`): there a sidecar that cannot
/// speak for the archives is an error, never a fallback.
pub(super) fn compilation_gc_effects(
    link_set: EffectsLinkSet,
    target: Option<&str>,
    dev: bool,
    verbose: u8,
) -> Result<Arc<GcEffects>> {
    let with_stdlib = match link_set {
        EffectsLinkSet::Undescribed(why) => {
            if verbose > 0 {
                eprintln!("  gc effects: {why}; all-unsafe table");
            }
            return Ok(Arc::new(GcEffects::all_unsafe()));
        }
        EffectsLinkSet::Prebuilt { with_stdlib } => with_stdlib,
    };
    // No runtime archive at all: there is no link set to bind a table to. A
    // linking compile fails at the link with the archive search report; a
    // `--no-link` compile (codegen-only census, IR tests) proves nothing about
    // runtime calls.
    let Ok(runtime) = locate_runtime_library(target) else {
        if verbose > 0 {
            eprintln!("  gc effects: no runtime archive located; all-unsafe table");
        }
        return Ok(Arc::new(GcEffects::all_unsafe()));
    };
    let stdlib = if with_stdlib {
        locate_stdlib_library(target)
    } else {
        None
    };
    let t = std::time::Instant::now();
    match read_verified(&runtime, stdlib.as_ref()) {
        Ok((table, combination)) => {
            let effects = GcEffects::from_sidecar(&table);
            if verbose > 0 {
                eprintln!(
                    "  gc effects: {} [{combination}] verified in {:.0} ms; {} proven rows",
                    sidecar_path(&runtime.candidate).display(),
                    t.elapsed().as_secs_f64() * 1e3,
                    effects.proven_rows(),
                );
            }
            Ok(Arc::new(effects))
        }
        Err(why) => unproven(dev, why, &runtime),
    }
}

/// The table of the sidecar beside `runtime` for the linked combination, once
/// every archive's bytes match its key.
fn read_verified(
    runtime: &LocatedLibrary,
    stdlib: Option<&LocatedLibrary>,
) -> std::result::Result<(perry_gc_effects::Table, &'static str), String> {
    let path = sidecar_path(&runtime.candidate);
    if !path.is_file() {
        return Err(format!("{} does not exist", path.display()));
    }
    let sidecar = Sidecar::read(&path).map_err(|e| format!("{e:#}"))?;
    // Hash the bytes the linker gets (the decompression slot of a `.a.zst`),
    // both archives at once.
    let links: Vec<&Path> = std::iter::once(runtime.link.as_path())
        .chain(stdlib.map(|s| s.link.as_path()))
        .collect();
    let keys: Vec<ArchiveKey> = std::thread::scope(|scope| {
        let handles: Vec<_> = links
            .iter()
            .map(|p| scope.spawn(move || archive_key(p)))
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("archive hash thread"))
            .collect::<Result<Vec<_>>>()
    })
    .map_err(|e| format!("{e:#}"))?;
    let combination = combination_name(stdlib.is_some());
    let pairs: Vec<(&Path, &ArchiveKey)> = links.iter().copied().zip(keys.iter()).collect();
    match sidecar.verified_table(combination, &pairs) {
        Ok(table) => Ok((table.clone(), combination)),
        Err(m @ Mismatch::Bytes { .. }) => Err(format!(
            "{} does not describe these archives: {m}",
            path.display()
        )),
        Err(m) => Err(format!("{}: {m}", path.display())),
    }
}

/// No sidecar speaks for the archives: an error from the workspace, the
/// all-unsafe table plus one warning from an installed perry.
fn unproven(dev: bool, why: String, runtime: &LocatedLibrary) -> Result<Arc<GcEffects>> {
    if dev {
        let dir = runtime
            .candidate
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        bail!(
            "the runtime GC effects sidecar cannot speak for the archives this program links:\n  \
             {why}\n\
             perry runs from its workspace, where every runtime archive must carry a \
             sidecar computed from its bytes. After building the archives, run:\n  \
             scripts/gc_call_effects/sidecars.sh {}",
            dir.display()
        );
    }
    eprintln!(
        "warning: no GC effects table for the runtime archives ({why}); compiling with every \
         runtime call treated as a safepoint that can throw (correct, slower). Reinstall perry \
         to restore the shipped `.gc-effects` files."
    );
    Ok(Arc::new(GcEffects::all_unsafe()))
}
