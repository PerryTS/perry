//! fninfer (owner decision 140) in the driver: run the per-function
//! signature inference over the final HIR of every module and write the
//! precision dump.
//!
//! Knobs:
//! - `PERRY_FNINFER_DUMP=<path>`: run the pass and write one TSV line per
//!   function (`F key module func_id name kind params ret this
//!   params_concrete hot_params_concrete prop_ops_concrete arith_concrete`)
//!   plus per-module wall time (`M module ms rounds`);
//! - `PERRY_FNINFER_ONLY=1`: exit after writing the dump (analysis-only);
//! - `PERRY_FNINFER_STRICT=1`: strict mode (field reads and unresolved call
//!   results are `Any`).

use std::collections::BTreeMap;
use std::path::PathBuf;

pub(crate) fn dump(modules: &BTreeMap<PathBuf, perry_hir::Module>) {
    let Some(path) = std::env::var_os("PERRY_FNINFER_DUMP") else {
        return;
    };
    let cfg = perry_hir::analysis::fn_infer::InferConfig {
        strict: std::env::var("PERRY_FNINFER_STRICT").is_ok_and(|v| v == "1"),
    };
    let started = std::time::Instant::now();
    let mut out = String::new();
    for (p, m) in modules {
        let t0 = std::time::Instant::now();
        let inf = perry_hir::analysis::fn_infer::infer_module(m, cfg);
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        let name = p.display().to_string();
        perry_hir::analysis::fn_infer::dump_module(&name, m, &inf, &mut out);
        out.push_str(&format!("M\t{name}\t{ms:.1}\t{}\n", inf.rounds));
    }
    out.push_str(&format!(
        "T\t{:.1}\n",
        started.elapsed().as_secs_f64() * 1000.0
    ));
    let _ = std::fs::write(PathBuf::from(path), out);
    if std::env::var("PERRY_FNINFER_ONLY").is_ok_and(|v| v == "1") {
        std::process::exit(0);
    }
}

/// `PERRY_FNINFER_APPLY=scalars|classes`: write inferred parameter types
/// into every module's HIR before codegen, where the declared type is
/// `any`. Codegen treats them like TypeScript annotations: candidates that
/// the guarded clone machinery proves at entry. Off by default.
pub(crate) fn apply(modules: &mut BTreeMap<PathBuf, perry_hir::Module>) {
    use perry_hir::analysis::fn_infer::{
        apply_param_types, infer_module, ApplyPolicy, InferConfig,
    };
    let policy = match std::env::var("PERRY_FNINFER_APPLY").as_deref() {
        Ok("scalars") => ApplyPolicy::Scalars,
        Ok("classes") => ApplyPolicy::ScalarsAndClasses,
        _ => return,
    };
    let started = std::time::Instant::now();
    let mut typed = 0usize;
    for m in modules.values_mut() {
        let inf = infer_module(m, InferConfig::default());
        typed += apply_param_types(m, &inf, policy);
    }
    if std::env::var("PERRY_FNINFER_REPORT").is_ok_and(|v| v == "1") {
        eprintln!(
            "[fninfer] typed {typed} parameters in {:.1} ms",
            started.elapsed().as_secs_f64() * 1000.0
        );
    }
}
