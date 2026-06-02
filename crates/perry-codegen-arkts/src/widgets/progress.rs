// This module is part of the perry-codegen-arkts crate. It was
// mechanically split out of the former monolithic lib.rs (issue
// #1100). Pure code move — no logic changes.
#![allow(clippy::too_many_arguments)]
use crate::*;

/// `ProgressView(value?, total?)` → ArkUI `Progress({value, total, type: ProgressType.Linear})`.
/// Defaults: value=0, total=100. Both args optional — leaf widget, no
/// callbacks, no children.
pub(crate) fn emit_progressview(args: &[Expr]) -> String {
    let value = numeric_arg(args, 0).unwrap_or(0.0);
    let total = numeric_arg(args, 1).unwrap_or(100.0);
    format!(
        "Progress({{ value: {value}, total: {total}, type: ProgressType.Linear }})",
        value = fmt_num(value),
        total = fmt_num(total),
    )
}
