//! Unknown `perry/ui` free-function calls met while lowering one module.
//!
//! Lowering records each one and keeps going, so a module that calls several
//! retired or misspelled functions fails once with all of them instead of
//! stopping at the first. `finish_module` must run before the module's object
//! is produced, so a recorded call never reaches the linker.
use std::cell::RefCell;

thread_local! {
    static UNKNOWN: RefCell<Vec<(String, usize)>> = const { RefCell::new(Vec::new()) };
}

pub(crate) fn begin_module() {
    UNKNOWN.with(|u| u.borrow_mut().clear());
}

/// Records `method` once per module; typed clones lower a body more than once.
pub(crate) fn record(method: &str, args: usize) {
    UNKNOWN.with(|u| {
        let mut u = u.borrow_mut();
        if !u.iter().any(|(m, _)| m == method) {
            u.push((method.to_string(), args));
        }
    });
}

pub(crate) fn finish_module() -> anyhow::Result<()> {
    let unknown = UNKNOWN.with(|u| std::mem::take(&mut *u.borrow_mut()));
    if unknown.is_empty() {
        return Ok(());
    }
    let mut msg = match unknown.len() {
        1 => "perry/ui: 1 unknown function:".to_string(),
        n => format!("perry/ui: {n} unknown functions:"),
    };
    for (method, args) in &unknown {
        msg.push_str(&format!("\n  '{method}' (args: {args})"));
        if let Some(hint) = retired_replacement(method) {
            msg.push_str(&format!(" was removed; use {hint}"));
        }
    }
    msg.push_str(
        "\nCheck the spelling and consult types/perry/ui/index.d.ts for the supported API surface.",
    );
    anyhow::bail!(msg)
}

/// Free functions the pre-v0.5.0 compiler accepted, mapped to the call that
/// replaces each. Only entries with a direct equivalent belong here.
fn retired_replacement(method: &str) -> Option<&'static str> {
    Some(match method {
        "widgetSetCornerRadius" => "`setCornerRadius(widget, radius)`",
        "setEdgeInsets" => "`setPadding(widget, top, left, bottom, right)`",
        "canvasClear" => "`canvas.clearRect(0, 0, width, height)`",
        "canvasBeginPath" => "`canvas.beginPath()`",
        "canvasMoveTo" => "`canvas.moveTo(x, y)`",
        "canvasLineTo" => "`canvas.lineTo(x, y)`",
        "canvasStroke" => {
            "`canvas.setStrokeColor(r, g, b, a)`, `canvas.setLineWidth(width)` and `canvas.stroke()`"
        }
        "canvasFillGradient" => "`canvas.fillGradient(r1, g1, b1, a1, r2, g2, b2, a2, direction)`",
        "appOnTerminate" => "`onTerminate(callback)`",
        "appOnActivate" => "`onActivate(callback)`",
        "Alert" => "`alert(title, message)` or `alertWithButtons(...)`",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_every_unknown_call_once_with_replacements() {
        begin_module();
        record("canvasClear", 1);
        record("bogusWidget", 0);
        record("canvasClear", 1);
        record("widgetSetCornerRadius", 2);
        let msg = finish_module().unwrap_err().to_string();
        assert!(msg.starts_with("perry/ui: 3 unknown functions:"), "{msg}");
        assert_eq!(msg.matches("'canvasClear'").count(), 1, "{msg}");
        assert!(
            msg.contains(
                "'canvasClear' (args: 1) was removed; use `canvas.clearRect(0, 0, width, height)`"
            ),
            "{msg}"
        );
        assert!(msg.contains("'bogusWidget' (args: 0)\n"), "{msg}");
        assert!(
            msg.contains("use `setCornerRadius(widget, radius)`"),
            "{msg}"
        );
        assert!(
            finish_module().is_ok(),
            "finish_module drains the module's calls"
        );
    }

    #[test]
    fn begin_module_drops_an_abandoned_module() {
        record("canvasClear", 1);
        begin_module();
        assert!(finish_module().is_ok());
    }
}
