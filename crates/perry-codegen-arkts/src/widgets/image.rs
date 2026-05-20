// This module is part of the perry-codegen-arkts crate. It was
// mechanically split out of the former monolithic lib.rs (issue
// #1100). Pure code move — no logic changes.
#![allow(clippy::too_many_arguments)]
use crate::*;

/// `Image(src)` / `ImageFile(src)` → `Image('src').width('100%').height(200)`.
/// Default sizing matches the perry-ui-* native default of "fill width,
/// 200pt tall"; users can wrap in further sizing via container modifiers
/// later (Phase 2 v5 will likely accept a `style: { ... }` trailing arg).
///
/// Resolves the src arg through `bindings` so common patterns work:
/// - `ImageFile('assets/icon.png')` — direct literal
/// - `ImageFile(LOGO_PATH)` where `const LOGO_PATH = 'assets/...'`
/// - `ImageFile(mobile ? 'path-mobile' : 'path-desktop')` — ternary,
///   evaluates the condition when foldable, otherwise picks the
///   then-branch (mirrors the `Expr::Conditional` widget heuristic).
///
/// Falls back to a placeholder Text only when the chain bottoms out
/// at a non-string leaf (function call, prop access, etc.).
pub(crate) fn emit_image(args: &[Expr], bindings: &HashMap<LocalId, Expr>) -> String {
    let Some(first) = args.first() else {
        return "Text('[non-literal Image src]').fontSize(16).fontColor('#888888')".to_string();
    };
    let Some(src) = resolve_string_arg(first, bindings) else {
        return "Text('[non-literal Image src]').fontSize(16).fontColor('#888888')".to_string();
    };
    // Phase 2 v13 — recognize the `@app.media/<name>` resource path
    // shape and emit ArkUI's `$r('app.media.<name>')` accessor instead
    // of a quoted string literal. Plain URLs / file paths still pass
    // through as quoted strings.
    //
    // `assets/X.png` paths (Mango's convention) translate to
    // `$rawfile('X.png')` — the HAP build (`harmonyos_hap.rs::copy_
    // assets_to_rawfile`) copies the project's `assets/` directory
    // verbatim into `resources/rawfile/`, and ArkUI's Image accepts
    // `$rawfile()` for raw resource references.
    let src_arg = if let Some(name) = src.strip_prefix("@app.media/") {
        // ArkUI's $r() takes a dot-path string, NOT a slash-path.
        format!("$r('app.media.{}')", name)
    } else if let Some(name) = src.strip_prefix("@app.icon/") {
        format!("$r('app.icon.{}')", name)
    } else if let Some(rest) = src.strip_prefix("assets/") {
        format!("$rawfile('{}')", rest)
    } else {
        arkts_string_lit(&src)
    };
    format!("Image({}).width('100%').height(200)", src_arg)
}

/// Walk a string-typed argument through bindings + ternary branches to
/// find the underlying string literal. Returns None when the chain
/// bottoms out at a non-string leaf. Same shape as
/// `numeric_arg_resolved` but for strings.
pub(crate) fn resolve_string_arg(expr: &Expr, bindings: &HashMap<LocalId, Expr>) -> Option<String> {
    let mut cur = expr;
    for _ in 0..16 {
        match cur {
            Expr::String(s) => return Some(s.clone()),
            Expr::LocalGet(id) => {
                cur = bindings.get(id)?;
            }
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                // Same heuristic as the widget Conditional emit: if the
                // condition const-folds, pick the corresponding branch;
                // otherwise default to the then-branch (the "primary"
                // case the author wrote first).
                cur = match evaluate_condition(condition, bindings, &HashMap::new()) {
                    Some(false) => else_expr,
                    _ => then_expr,
                };
            }
            // `t('key')` from `perry/i18n` lowers to an
            // `Expr::I18nString { key, ... }`. Use the key as the
            // string fallback — for Mango (and most apps using Perry's
            // i18n) the English source text doubles as the key, so
            // emitting the key gives the user readable English text on
            // platforms where dynamic i18n hasn't been wired yet.
            // Future: thread a locale lookup table through the harvest
            // and pick the matching translation.
            Expr::I18nString { key, .. } => return Some(key.clone()),
            // `t('key')` may also surface as a NativeMethodCall to
            // `perry/i18n` if the caller used the destructured-import
            // form. Unwrap to the inner I18nString (or plain string)
            // arg.
            Expr::NativeMethodCall {
                module,
                method,
                args,
                ..
            } if module == "perry/i18n" && method == "t" => {
                cur = args.first()?;
            }
            _ => return None,
        }
    }
    None
}
