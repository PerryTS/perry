//! Export demand: compiled programs keep no declaration that only unused
//! exports reach (layer 1 of dead-code elimination, module/export level, as
//! rollup does it; sound without facts).
//!
//! The first collection walk summarizes each module's top-level items
//! (`summary`): what each declares, which module-scope names it mentions,
//! and whether evaluating it can do anything but create its bindings
//! (`Eval`, by structure: declarations, literals, functions, classes whose
//! heritage, keys and static parts are inert). Between the walks `plan`
//! follows the demand from the graph roots through every import, re-export
//! and `export *` by name. The second walk (`rerun_collect_with_class_field_types`)
//! drops each dead item from the AST before lowering (`prune`), so facts and
//! codegen never see it. Every module still evaluates, in the same order,
//! with every effectful item.
//!
//! `PERRY_EXPORT_DEMAND=off` keeps everything (A/B measurement);
//! `PERRY_EXPORT_DEMAND_REPORT=<path>` writes the per-module counts.

mod plan;
mod prune;
mod summary;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use anyhow::{bail, Result};
use swc_ecma_ast as ast;

use super::CompilationContext;

pub(crate) use plan::Prune;
pub(crate) use summary::Summary;

/// The walk state: summaries during the first walk, the plan in the second.
#[derive(Default)]
pub(crate) struct ExportDemand {
    summaries: HashMap<PathBuf, Summary>,
    plan: Option<BTreeMap<PathBuf, Prune>>,
    /// Modules whose second-walk body differs from the summarized one.
    changed: Vec<PathBuf>,
}

fn enabled() -> bool {
    !matches!(
        std::env::var("PERRY_EXPORT_DEMAND").as_deref(),
        Ok("off" | "0" | "false" | "no")
    )
}

/// Called for every module the walk lowers, with the AST lowering receives.
/// First walk: records the summary. Second walk: the pruned AST, if any.
pub(super) fn visit(
    ctx: &mut CompilationContext,
    canonical: &Path,
    module: &ast::Module,
    cjs_wrapped: bool,
) -> Option<ast::Module> {
    if !enabled() {
        return None;
    }
    let state = &mut ctx.export_demand;
    match &state.plan {
        None => {
            state.summaries.insert(
                canonical.to_path_buf(),
                summary::summarize(module, cjs_wrapped),
            );
            None
        }
        Some(plan) => {
            let prune = plan.get(canonical)?;
            if summary::fingerprint(module) != prune.fingerprint {
                state.changed.push(canonical.to_path_buf());
                return None;
            }
            Some(prune::prune(module, prune))
        }
    }
}

/// Between the walks: the plan. True when the second walk prunes anything.
pub(super) fn make_plan(ctx: &mut CompilationContext, roots: &[PathBuf]) -> Result<bool> {
    if !enabled() {
        return Ok(false);
    }
    let summaries = std::mem::take(&mut ctx.export_demand.summaries);
    let planned = plan::plan(ctx, &summaries, roots);
    let Some(planned) = planned else {
        eprintln!(
            "perry: export demand: an import of a compiled module did not resolve; nothing pruned"
        );
        return Ok(false);
    };
    if let Some(path) = std::env::var_os("PERRY_EXPORT_DEMAND_REPORT") {
        std::fs::write(&path, planned.report.join("\n") + "\n")?;
    }
    let any = !planned.prunes.is_empty();
    ctx.export_demand.plan = Some(planned.prunes);
    Ok(any)
}

/// After the second walk: a module the plan pruned others for must have
/// lowered the body that was summarized.
pub(super) fn check_walks(ctx: &CompilationContext) -> Result<()> {
    if let Some(p) = ctx.export_demand.changed.first() {
        bail!(
            "export demand: {} changed between the collection walks; set PERRY_EXPORT_DEMAND=off and report this",
            p.display()
        );
    }
    Ok(())
}
