// This module is part of the perry-codegen-arkts crate. It was
// mechanically split out of the former monolithic lib.rs (issue
// #1100). Pure code move — no logic changes.
#![allow(clippy::too_many_arguments)]
use crate::*;

/// `Text("hi")` → `Text('hi').fontSize(20)`.
///
/// Phase 2 v3 Option 2: `Text("hi", "id")` → registers a reactive slot.
/// The widget emits `Text(this.text_<id>)` instead of a string literal,
/// and `wrap_index_page` adds `@State text_<id>: string = 'hi'` to the
/// page struct. User code calls `setText("id", newValue)` from inside
/// a closure to update.
///
/// Non-string-literal args fall back to a placeholder so unsupported
/// shapes don't break the build.
pub(crate) fn emit_text(
    args: &[Expr],
    text_slots: &mut Vec<TextSlot>,
    arkts_locals: &HashMap<LocalId, String>,
    bindings: &HashMap<LocalId, Expr>,
) -> String {
    // Phase 2 v5: inside a ForEach body, `Text(item)` where `item` is
    // the closure's loop param resolves via arkts_locals → `Text(__item)`.
    let first = args.first();
    let content_str = match first {
        Some(Expr::String(content)) => Some(arkts_string_lit(content)),
        Some(Expr::LocalGet(id)) if arkts_locals.contains_key(id) => arkts_locals.get(id).cloned(),
        // Fallback: try to resolve through bindings + Conditional +
        // I18nString (perry/i18n's `t('key')` lowers to I18nString).
        // This catches Mango's `Text(t('Welcome to Mango'))` shape.
        Some(other) => resolve_string_arg(other, bindings).map(|s| arkts_string_lit(&s)),
        None => None,
    };
    let Some(content_arg) = content_str else {
        return "Text('[non-literal Text arg]').fontSize(20).fontColor('#888888')".to_string();
    };
    if let Some(Expr::String(id)) = args.get(1) {
        // Reactive Text. Sanitize the id so it's a valid ArkTS field-
        // name suffix (alphanumeric + underscore). The original id stays
        // alongside it for the runtime-side switch match.
        // Only the literal-string form is reactive — ForEach's __item
        // binding is per-iteration and doesn't persist to a slot.
        if let Some(Expr::String(initial)) = first {
            let safe = sanitize_text_id(id);
            text_slots.push(TextSlot {
                original_id: id.clone(),
                field_id: safe.clone(),
                initial: initial.clone(),
            });
            return format!("Text(this.text_{}).fontSize(20)", safe);
        }
    }
    format!("Text({}).fontSize(20)", content_arg)
}

/// Extract a `style: {...}` object from a widget arg. Handles both
/// `Expr::Object(props)` (open shape) and Perry's closed-shape
/// optimization `Expr::New { class_name: "__AnonShape_*", args }` where
/// the class's fields list correlates positionally with args. Used by
/// `emit_style_modifiers` to map StyleProps into ArkUI modifiers.
///
/// Phase 2 v5 — ergonomic parity with macOS/iOS/etc inline styling.
pub(crate) fn extract_style_object(arg: &Expr, classes: &[Class]) -> Option<Vec<(String, Expr)>> {
    match arg {
        Expr::Object(props) => Some(props.clone()),
        Expr::New {
            class_name, args, ..
        } if class_name.starts_with("__AnonShape_") => {
            let class = classes.iter().find(|c| &c.name == class_name)?;
            // Pair each field with its positional arg; missing args fall through.
            let pairs: Vec<(String, Expr)> = class
                .fields
                .iter()
                .enumerate()
                .filter_map(|(i, f)| args.get(i).map(|a| (f.name.clone(), a.clone())))
                .collect();
            Some(pairs)
        }
        _ => None,
    }
}

/// Map a Perry color expression to an ArkUI color string.
///   - `Expr::String("blue")` / `"#3B82F6"` → quoted string passthrough
///   - `Expr::Object([(r,…),(g,…),(b,…),(a,…)])` (PerryColor) → `'rgba(R,G,B,A)'`
///     where channels are scaled to 0..255 / 0..1 per CSS rgba() convention
pub(crate) fn arkts_color_value(e: &Expr) -> String {
    match e {
        Expr::String(s) => arkts_string_lit(s),
        Expr::Object(props) => {
            let chan = |name: &str, default: f64| -> f64 {
                props
                    .iter()
                    .find(|(k, _)| k == name)
                    .and_then(|(_, v)| match v {
                        Expr::Number(n) => Some(*n),
                        Expr::Integer(n) => Some(*n as f64),
                        _ => None,
                    })
                    .unwrap_or(default)
            };
            let r = (chan("r", 0.0) * 255.0).round() as i64;
            let g = (chan("g", 0.0) * 255.0).round() as i64;
            let b = (chan("b", 0.0) * 255.0).round() as i64;
            let a = chan("a", 1.0);
            format!("'rgba({}, {}, {}, {})'", r, g, b, fmt_num(a))
        }
        _ => "'#000000'".to_string(),
    }
}

/// Phase 2 v13 — map a CSS-style curve string to ArkUI's `Curve` enum.
/// ArkUI `Curve` lives at `@ohos.curves` and the values match the W3C
/// timing-function names with PascalCase (`Curve.Linear`, `Curve.Ease`,
/// `Curve.EaseInOut`, etc.). Unrecognized values fall back to `Curve.Ease`.
pub(crate) fn arkts_curve_value(s: &str) -> String {
    let name = match s {
        "linear" => "Linear",
        "ease" => "Ease",
        "ease-in" | "easeIn" => "EaseIn",
        "ease-out" | "easeOut" => "EaseOut",
        "ease-in-out" | "easeInOut" => "EaseInOut",
        "fast-out-slow-in" => "FastOutSlowIn",
        "linear-out-slow-in" => "LinearOutSlowIn",
        "fast-out-linear-in" => "FastOutLinearIn",
        "extreme-deceleration" => "ExtremeDeceleration",
        "sharp" => "Sharp",
        "rhythm" => "Rhythm",
        "smooth" => "Smooth",
        "friction" => "Friction",
        _ => "Ease",
    };
    format!("Curve.{}", name)
}

/// Map a `StyleProps` object to an ArkUI modifier chain like
/// `.backgroundColor('blue').borderRadius(8).opacity(0.95)`.
///
/// Phase 2 v5 covers the high-traffic props: backgroundColor, color,
/// fontSize, fontWeight, fontFamily, borderRadius, padding, opacity,
/// hidden, borderColor + borderWidth (as combined `.border({...})`).
/// Skipped (complex / multi-arg ArkUI shape): shadow, gradient,
/// textDecoration, tooltip, animation, transition — these would each
/// need their own ArkUI modifier and are deferred to Phase 2 v13.
pub(crate) fn emit_style_modifiers(props: &[(String, Expr)]) -> String {
    let mut out = String::new();
    let mut border_color: Option<String> = None;
    let mut border_width: Option<String> = None;
    for (k, v) in props {
        match k.as_str() {
            "backgroundColor" => {
                out.push_str(&format!(".backgroundColor({})", arkts_color_value(v)));
            }
            "color" => {
                // ArkUI's `.fontColor` works on Text; non-text widgets
                // silently ignore it.
                out.push_str(&format!(".fontColor({})", arkts_color_value(v)));
            }
            "fontSize" => {
                if let Some(n) = numeric_expr(v) {
                    out.push_str(&format!(".fontSize({})", fmt_num(n)));
                }
            }
            "fontWeight" => {
                if let Some(n) = numeric_expr(v) {
                    out.push_str(&format!(".fontWeight({})", fmt_num(n)));
                }
            }
            "fontFamily" => {
                if let Expr::String(s) = v {
                    out.push_str(&format!(".fontFamily({})", arkts_string_lit(s)));
                }
            }
            "borderRadius" => {
                if let Some(n) = numeric_expr(v) {
                    out.push_str(&format!(".borderRadius({})", fmt_num(n)));
                }
            }
            "borderColor" => {
                border_color = Some(arkts_color_value(v));
            }
            "borderWidth" => {
                if let Some(n) = numeric_expr(v) {
                    border_width = Some(fmt_num(n));
                }
            }
            "padding" => match v {
                Expr::Number(n) => out.push_str(&format!(".padding({})", fmt_num(*n))),
                Expr::Integer(n) => out.push_str(&format!(".padding({})", *n)),
                Expr::Object(sides) => {
                    let side = |name: &str| -> Option<f64> {
                        sides
                            .iter()
                            .find(|(k, _)| k == name)
                            .and_then(|(_, v)| numeric_expr(v))
                    };
                    let parts: Vec<String> = ["top", "right", "bottom", "left"]
                        .iter()
                        .filter_map(|s| side(s).map(|n| format!("{}: {}", s, fmt_num(n))))
                        .collect();
                    if !parts.is_empty() {
                        out.push_str(&format!(".padding({{ {} }})", parts.join(", ")));
                    }
                }
                _ => {}
            },
            "opacity" => {
                if let Some(n) = numeric_expr(v) {
                    out.push_str(&format!(".opacity({})", fmt_num(n)));
                }
            }
            "hidden" => {
                let is_hidden = matches!(v, Expr::Bool(true));
                if is_hidden {
                    out.push_str(".visibility(Visibility.Hidden)");
                }
            }
            // Phase 2 v13 — animation/transition/shadow/textDecoration.
            "animation" => {
                if let Expr::Object(props) = v {
                    let mut parts: Vec<String> = Vec::new();
                    for (k2, v2) in props {
                        match k2.as_str() {
                            "duration" => {
                                if let Some(n) = numeric_expr(v2) {
                                    parts.push(format!("duration: {}", fmt_num(n)));
                                }
                            }
                            "curve" => {
                                if let Expr::String(s) = v2 {
                                    parts.push(format!("curve: {}", arkts_curve_value(s)));
                                }
                            }
                            "delay" => {
                                if let Some(n) = numeric_expr(v2) {
                                    parts.push(format!("delay: {}", fmt_num(n)));
                                }
                            }
                            "iterations" => {
                                if let Some(n) = numeric_expr(v2) {
                                    parts.push(format!("iterations: {}", fmt_num(n)));
                                }
                            }
                            _ => {}
                        }
                    }
                    if !parts.is_empty() {
                        out.push_str(&format!(".animation({{ {} }})", parts.join(", ")));
                    }
                }
            }
            "shadow" => {
                if let Expr::Object(props) = v {
                    let mut parts: Vec<String> = Vec::new();
                    for (k2, v2) in props {
                        match k2.as_str() {
                            "color" => {
                                parts.push(format!("color: {}", arkts_color_value(v2)));
                            }
                            "blur" => {
                                if let Some(n) = numeric_expr(v2) {
                                    parts.push(format!("radius: {}", fmt_num(n)));
                                }
                            }
                            "offsetX" => {
                                if let Some(n) = numeric_expr(v2) {
                                    parts.push(format!("offsetX: {}", fmt_num(n)));
                                }
                            }
                            "offsetY" => {
                                if let Some(n) = numeric_expr(v2) {
                                    parts.push(format!("offsetY: {}", fmt_num(n)));
                                }
                            }
                            _ => {}
                        }
                    }
                    if !parts.is_empty() {
                        out.push_str(&format!(".shadow({{ {} }})", parts.join(", ")));
                    }
                }
            }
            "textDecoration" => {
                if let Expr::String(s) = v {
                    let kind = match s.as_str() {
                        "underline" => Some("Underline"),
                        "strikethrough" | "line-through" => Some("LineThrough"),
                        "overline" => Some("Overline"),
                        "none" => Some("None"),
                        _ => None,
                    };
                    if let Some(k) = kind {
                        out.push_str(&format!(
                            ".decoration({{ type: TextDecorationType.{} }})",
                            k
                        ));
                    }
                }
            }
            // Phase 2 v13 deferred: gradient, transition, tooltip — these
            // each need more complex ArkUI shapes (linearGradient, multi-
            // part transition config, custom-component popup) and are
            // tracked as v13.5 follow-ups.
            _ => {}
        }
    }
    // Joint border: ArkUI's `.border({color, width})` is one modifier
    // taking a config object; emit only if at least one was set.
    if border_color.is_some() || border_width.is_some() {
        let mut parts: Vec<String> = Vec::new();
        if let Some(w) = border_width {
            parts.push(format!("width: {}", w));
        }
        if let Some(c) = border_color {
            parts.push(format!("color: {}", c));
        }
        out.push_str(&format!(".border({{ {} }})", parts.join(", ")));
    }
    out
}

/// Extract a Number / Integer expression as `f64`. Returns None for
/// anything else (including `Expr::String` parseable numerals — those
/// are intentionally rejected because StyleProps forbids them).
pub(crate) fn numeric_expr(e: &Expr) -> Option<f64> {
    match e {
        Expr::Number(n) => Some(*n),
        Expr::Integer(n) => Some(*n as f64),
        _ => None,
    }
}

/// Sanitize an arbitrary string id into a valid ArkTS field-name suffix.
/// Replaces non-[a-zA-Z0-9_] with `_`. Front-pads with `x` if it starts
/// with a digit. Empty input → `default`.
pub(crate) fn sanitize_text_id(s: &str) -> String {
    if s.is_empty() {
        return "default".to_string();
    }
    let mut out: String = s
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if out
        .chars()
        .next()
        .map(|c| c.is_ascii_digit())
        .unwrap_or(false)
    {
        out.insert(0, 'x');
    }
    out
}
