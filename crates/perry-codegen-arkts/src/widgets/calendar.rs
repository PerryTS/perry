// This module is part of the perry-codegen-arkts crate. It was
// mechanically split out of the former monolithic lib.rs (issue
// #1100). Pure code move — no logic changes.
#![allow(clippy::too_many_arguments)]
use crate::*;

/// Issue #481 — `Calendar(year, month, onChange)` → ArkUI
/// `CalendarPicker({selected: new Date(year, month-1, 1)}).onChange(...)`.
/// CalendarPicker fires `.onChange((value: Date) => ...)` when the user
/// picks a date. Perry's `onChange` receives an ISO `yyyy-MM-dd` string
/// (POSIX-locale), so the forwarded payload is `value.toISOString().split('T')[0]`.
///
/// JavaScript's `Date` constructor takes `(year, monthIndex, day)` where
/// monthIndex is 0-based; Perry passes a 1-based month per its TS
/// signature, so we emit `month - 1` literally when both args resolve.
/// When the args don't resolve to literals, we fall back to today's
/// date (`new Date()`) — the user can still pick a date even if the
/// initial highlight is wrong, which is strictly better than emitting
/// an invalid Date object.
pub(crate) fn emit_calendar(args: &[Expr], callbacks: &mut Vec<Expr>) -> String {
    let year = numeric_arg(args, 0);
    let month = numeric_arg(args, 1);
    let selected = match (year, month) {
        (Some(y), Some(m)) => {
            // ArkUI's Date constructor wants (year, monthIndex, day).
            let m_idx = (m as i64).saturating_sub(1).max(0);
            format!("new Date({}, {}, 1)", y as i64, m_idx)
        }
        _ => "new Date()".to_string(),
    };

    let onchange = match args.get(2) {
        Some(closure @ Expr::Closure { .. }) => {
            let idx = callbacks.len();
            callbacks.push(closure.clone());
            // ArkUI's CalendarPicker.onChange hands us a `Date`; convert
            // to the ISO yyyy-MM-dd shape Perry's TS surface promises.
            // .toISOString() returns "yyyy-MM-ddTHH:mm:ss.sssZ" — split
            // at 'T' and take the date half.
            format!(
                ".onChange((value: Date) => {{\n    \
                 const __iso = value.toISOString().split('T')[0];\n    \
                 perryEntry.invokeCallback1({}, __iso);\n    \
                 {drain}\
                 }})",
                idx,
                drain = drain_loop_body()
            )
        }
        _ => String::new(),
    };

    format!(
        "CalendarPicker({{ selected: {sel} }}){onchange}",
        sel = selected,
        onchange = onchange,
    )
}
