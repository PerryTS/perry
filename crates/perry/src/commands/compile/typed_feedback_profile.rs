//! CLI freshness inputs for advisory typed-feedback replay.
use super::CompileArgs;
use anyhow::{Context, Result};
use perry_codegen::typed_feedback_profile::{ModuleIdentity, Session};
use sha2::{Digest, Sha256};
use std::path::Path;

pub(super) fn prepare(args: &CompileArgs) -> Result<Option<Session>> {
    if args.typed_feedback_profile.is_none() && args.typed_feedback_sites.is_none() {
        return Ok(None);
    }
    if matches!(
        args.target.as_deref(),
        Some(
            "web"
                | "wasm"
                | "ios-widget"
                | "ios-widget-simulator"
                | "watchos-widget"
                | "watchos-widget-simulator"
                | "android-widget"
                | "wearos-tile"
        )
    ) {
        anyhow::bail!("typed-feedback capture/replay requires a native LLVM target");
    }
    let profile = args
        .typed_feedback_profile
        .as_deref()
        .map(Session::read_profile)
        .transpose()?;
    // No version-only fallback: unreadable compiler identity is an actionable
    // error for explicit replay/capture, never permission to trust stale facts.
    let executable =
        std::env::current_exe().context("cannot identify compiler for typed-feedback replay")?;
    let compiler = format!(
        "sha256:{}",
        hex::encode(Sha256::digest(
            std::fs::read(&executable).context("cannot hash compiler for typed-feedback replay")?
        ))
    );
    Ok(Some(Session::new(compiler, profile)))
}

pub(super) fn compile(
    session: &Session,
    hir: &perry_hir::Module,
    opts: perry_codegen::CompileOptions,
    path: &Path,
    version: &str,
) -> Result<Vec<u8>> {
    let source = std::fs::read(path)
        .with_context(|| format!("cannot hash typed-feedback source {}", path.display()))?;
    let hir_hash = perry_hir::stable_hash::hash_module(hir);
    let identity = ModuleIdentity {
        module: hir.name.clone(),
        source_hash: format!("sha256:{}", hex::encode(Sha256::digest(&source))),
        hir_hash: format!("{hir_hash:016x}"),
        lowering_hash: format!(
            "{:016x}",
            super::object_cache::typed_feedback_lowering_key(&opts, hir_hash, version)
        ),
        target: perry_codegen::typed_feedback_profile::effective_target(&opts),
    };
    session.compile_module(hir, opts, identity)
}
