//! Inline `style: { ... }` destructure for perry/ui widget constructors.
//!
//! Extracted from `lower_call.rs` (Tier 2.2 of the compiler-improvement
//! plan, v0.5.333). The entry point `apply_inline_style` is called by
//! `lower_perry_ui_table_call` after a widget handle is created — it
//! destructures a trailing object-literal arg and emits the matching
//! `widgetSetXxx` setter calls.
//!
//! Issue #185 lineage: every helper here is the result of one Phase C
//! step (color destructure, padding shapes, shadow object, gradient,
//! runtime-fallback color parsing). They cluster together because they
//! all consume `extract_options_fields`-shaped objects.

use anyhow::Result;
use perry_hir::Expr;

use crate::expr::{lower_expr, unbox_to_i64, FnCtx};
use crate::types::{DOUBLE, I64};

// `extract_options_fields` and `get_raw_string_ptr` stay in
// `lower_call.rs` (the parent module) — they're used by other parts
// of native-method dispatch too.
use super::{extract_options_fields, get_raw_string_ptr};

pub(super) fn apply_inline_style(
    ctx: &mut FnCtx<'_>,
    handle: &str,
    style_arg: &Expr,
) -> Result<()> {
    let Some(props) = extract_options_fields(ctx, style_arg) else {
        // Not an object literal — silently skip rather than bail, so a
        // user passing `undefined` (no style) just gets the bare widget.
        return Ok(());
    };
    for (key, val) in &props {
        match key.as_str() {
            "borderRadius" => {
                let v = lower_expr(ctx, val)?;
                ctx.pending_declares.push((
                    "perry_ui_widget_set_corner_radius".to_string(),
                    DOUBLE,
                    vec![I64, DOUBLE],
                ));
                ctx.block().call(
                    DOUBLE,
                    "perry_ui_widget_set_corner_radius",
                    &[(I64, handle), (DOUBLE, &v)],
                );
            }
            "opacity" => {
                let v = lower_expr(ctx, val)?;
                ctx.pending_declares.push((
                    "perry_ui_widget_set_opacity".to_string(),
                    DOUBLE,
                    vec![I64, DOUBLE],
                ));
                ctx.block().call(
                    DOUBLE,
                    "perry_ui_widget_set_opacity",
                    &[(I64, handle), (DOUBLE, &v)],
                );
            }
            "borderWidth" => {
                let v = lower_expr(ctx, val)?;
                ctx.pending_declares.push((
                    "perry_ui_widget_set_border_width".to_string(),
                    DOUBLE,
                    vec![I64, DOUBLE],
                ));
                ctx.block().call(
                    DOUBLE,
                    "perry_ui_widget_set_border_width",
                    &[(I64, handle), (DOUBLE, &v)],
                );
            }
            "tooltip" => {
                let s = get_raw_string_ptr(ctx, val)?;
                ctx.pending_declares.push((
                    "perry_ui_widget_set_tooltip".to_string(),
                    DOUBLE,
                    vec![I64, I64],
                ));
                ctx.block().call(
                    DOUBLE,
                    "perry_ui_widget_set_tooltip",
                    &[(I64, handle), (I64, &s)],
                );
            }
            "hidden" => {
                let v = lower_expr(ctx, val)?;
                let blk = ctx.block();
                let bits = unbox_to_i64(blk, &v);
                ctx.pending_declares.push((
                    "perry_ui_set_widget_hidden".to_string(),
                    DOUBLE,
                    vec![I64, I64],
                ));
                ctx.block().call(
                    DOUBLE,
                    "perry_ui_set_widget_hidden",
                    &[(I64, handle), (I64, &bits)],
                );
            }
            "enabled" => {
                let v = lower_expr(ctx, val)?;
                let blk = ctx.block();
                let bits = unbox_to_i64(blk, &v);
                ctx.pending_declares.push((
                    "perry_ui_widget_set_enabled".to_string(),
                    DOUBLE,
                    vec![I64, I64],
                ));
                ctx.block().call(
                    DOUBLE,
                    "perry_ui_widget_set_enabled",
                    &[(I64, handle), (I64, &bits)],
                );
            }
            // Issue #185 Phase C step 3: multi-arg destructure for
            // color, padding-object, and shadow. PerryColor object
            // literals get destructured to (r, g, b, a) at HIR time;
            // anything else (string colors, runtime expressions) falls
            // through to the catch-all and is silently skipped — step 4
            // will add runtime parseColor + dynamic-value paths.
            "backgroundColor" => {
                let (r, g, b, a) = lower_color_with_runtime_fallback(ctx, val)?;
                ctx.pending_declares.push((
                    "perry_ui_widget_set_background_color".to_string(),
                    DOUBLE,
                    vec![I64, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
                ));
                ctx.block().call(
                    DOUBLE,
                    "perry_ui_widget_set_background_color",
                    &[
                        (I64, handle),
                        (DOUBLE, &r),
                        (DOUBLE, &g),
                        (DOUBLE, &b),
                        (DOUBLE, &a),
                    ],
                );
            }
            "color" => {
                // For most widgets `text_set_color` is the right setter;
                // Button has its own button_set_text_color. Default to
                // the generic textSet path — works on Text and is a no-op
                // on widgets that ignore it.
                let (r, g, b, a) = lower_color_with_runtime_fallback(ctx, val)?;
                ctx.pending_declares.push((
                    "perry_ui_text_set_color".to_string(),
                    DOUBLE,
                    vec![I64, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
                ));
                ctx.block().call(
                    DOUBLE,
                    "perry_ui_text_set_color",
                    &[
                        (I64, handle),
                        (DOUBLE, &r),
                        (DOUBLE, &g),
                        (DOUBLE, &b),
                        (DOUBLE, &a),
                    ],
                );
            }
            "borderColor" => {
                let (r, g, b, a) = lower_color_with_runtime_fallback(ctx, val)?;
                ctx.pending_declares.push((
                    "perry_ui_widget_set_border_color".to_string(),
                    DOUBLE,
                    vec![I64, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
                ));
                ctx.block().call(
                    DOUBLE,
                    "perry_ui_widget_set_border_color",
                    &[
                        (I64, handle),
                        (DOUBLE, &r),
                        (DOUBLE, &g),
                        (DOUBLE, &b),
                        (DOUBLE, &a),
                    ],
                );
            }
            "padding" => {
                let (top, right, bottom, left) = match val {
                    // Single number → all 4 sides. Match both `Number`
                    // (f64 literal) and `Integer` (i64 literal — Perry
                    // distinguishes them).
                    Expr::Number(_) | Expr::Integer(_) => {
                        let v = lower_expr(ctx, val)?;
                        (v.clone(), v.clone(), v.clone(), v)
                    }
                    // Per-side object literal.
                    other => {
                        if let Some(sides) = extract_padding_sides(ctx, other)? {
                            sides
                        } else {
                            // Runtime expression — lower for side
                            // effects, defer setter emission.
                            let _ = lower_expr(ctx, val)?;
                            continue;
                        }
                    }
                };
                ctx.pending_declares.push((
                    "perry_ui_widget_set_edge_insets".to_string(),
                    DOUBLE,
                    vec![I64, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
                ));
                ctx.block().call(
                    DOUBLE,
                    "perry_ui_widget_set_edge_insets",
                    &[
                        (I64, handle),
                        (DOUBLE, &top),
                        (DOUBLE, &right),
                        (DOUBLE, &bottom),
                        (DOUBLE, &left),
                    ],
                );
            }
            "shadow" => {
                if let Some((cr, cg, cb, ca, blur, dx, dy)) = extract_shadow_obj(ctx, val)? {
                    ctx.pending_declares.push((
                        "perry_ui_widget_set_shadow".to_string(),
                        DOUBLE,
                        vec![I64, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE],
                    ));
                    ctx.block().call(
                        DOUBLE,
                        "perry_ui_widget_set_shadow",
                        &[
                            (I64, handle),
                            (DOUBLE, &cr),
                            (DOUBLE, &cg),
                            (DOUBLE, &cb),
                            (DOUBLE, &ca),
                            (DOUBLE, &blur),
                            (DOUBLE, &dx),
                            (DOUBLE, &dy),
                        ],
                    );
                }
            }
            "textDecoration" => {
                // 0=none, 1=underline, 2=strikethrough — TS surface uses
                // string literals, map them at HIR time.
                let n: i64 = match val {
                    Expr::String(s) if s == "underline" => 1,
                    Expr::String(s) if s == "strikethrough" => 2,
                    _ => 0,
                };
                ctx.pending_declares.push((
                    "perry_ui_text_set_decoration".to_string(),
                    DOUBLE,
                    vec![I64, I64],
                ));
                let n_str = n.to_string();
                ctx.block().call(
                    DOUBLE,
                    "perry_ui_text_set_decoration",
                    &[(I64, handle), (I64, &n_str)],
                );
            }
            "gradient" => {
                // Phase C step 6: `{ angle, stops: [c1, c2] }` →
                // `widgetSetBackgroundGradient(handle, r1, g1, b1, a1,
                //   r2, g2, b2, a2, angle)`. The runtime FFI is 2-color
                // only; if more stops are passed, we use the first two.
                if let Some((angle, c1, c2)) = extract_gradient_obj(ctx, val)? {
                    let (r1, g1, b1, a1) = c1;
                    let (r2, g2, b2, a2) = c2;
                    ctx.pending_declares.push((
                        "perry_ui_widget_set_background_gradient".to_string(),
                        DOUBLE,
                        vec![
                            I64, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE, DOUBLE,
                            DOUBLE,
                        ],
                    ));
                    ctx.block().call(
                        DOUBLE,
                        "perry_ui_widget_set_background_gradient",
                        &[
                            (I64, handle),
                            (DOUBLE, &r1),
                            (DOUBLE, &g1),
                            (DOUBLE, &b1),
                            (DOUBLE, &a1),
                            (DOUBLE, &r2),
                            (DOUBLE, &g2),
                            (DOUBLE, &b2),
                            (DOUBLE, &a2),
                            (DOUBLE, &angle),
                        ],
                    );
                }
            }
            _ => {
                // Unknown / not-yet-supported key (runtime expressions
                // for color, or other dynamic shapes). Lower for side
                // effects but skip setter emission.
                let _ = lower_expr(ctx, val)?;
            }
        }
    }
    Ok(())
}

/// Extract a `PerryColor` object literal `{r, g, b, a?}` into 4 lowered
/// expression strings. Returns `None` if `val` isn't an object literal
/// (e.g., a string color or runtime expression — those go through the
/// step-4 runtime parseColor path).
fn extract_perry_color(
    ctx: &mut FnCtx<'_>,
    val: &Expr,
) -> Result<Option<(String, String, String, String)>> {
    // Issue #185 Phase C step 6: string-literal color parsing at HIR
    // time. Hex (#RGB / #RGBA / #RRGGBB / #RRGGBBAA) and a few common
    // named colors lower directly to 4 baked-in float literals — no
    // runtime cost. Runtime expressions still fall through to step-7
    // territory.
    if let Expr::String(s) = val {
        if let Some(rgba) = parse_color_string(s) {
            return Ok(Some(rgba));
        }
        return Ok(None);
    }

    let Some(props) = extract_options_fields(ctx, val) else {
        return Ok(None);
    };
    let mut r = "0.0".to_string();
    let mut g = "0.0".to_string();
    let mut b = "0.0".to_string();
    let mut a = "1.0".to_string();
    for (key, v) in &props {
        let lowered = lower_expr(ctx, v)?;
        match key.as_str() {
            "r" => r = lowered,
            "g" => g = lowered,
            "b" => b = lowered,
            "a" => a = lowered,
            _ => {}
        }
    }
    Ok(Some((r, g, b, a)))
}

/// Parse a CSS color string at compile time (issue #185 Phase C step 6).
/// Supports `#RGB`, `#RGBA`, `#RRGGBB`, `#RRGGBBAA` hex forms + a small
/// set of named colors. Returns 4 channel values as f64-formatted
/// strings ready for direct emission in LLVM IR.
fn parse_color_string(s: &str) -> Option<(String, String, String, String)> {
    let lower = s.trim().to_ascii_lowercase();
    let named = match lower.as_str() {
        "white" => Some((1.0, 1.0, 1.0, 1.0)),
        "black" => Some((0.0, 0.0, 0.0, 1.0)),
        "red" => Some((1.0, 0.0, 0.0, 1.0)),
        "green" => Some((0.0, 0.502, 0.0, 1.0)),
        "blue" => Some((0.0, 0.0, 1.0, 1.0)),
        "yellow" => Some((1.0, 1.0, 0.0, 1.0)),
        "cyan" => Some((0.0, 1.0, 1.0, 1.0)),
        "magenta" => Some((1.0, 0.0, 1.0, 1.0)),
        "gray" | "grey" => Some((0.502, 0.502, 0.502, 1.0)),
        "transparent" => Some((0.0, 0.0, 0.0, 0.0)),
        _ => None,
    };
    if let Some((r, g, b, a)) = named {
        return Some((fmt_float(r), fmt_float(g), fmt_float(b), fmt_float(a)));
    }
    if let Some(hex) = lower.strip_prefix('#') {
        let parse_pair = |s: &str| u8::from_str_radix(s, 16).ok().map(|b| b as f64 / 255.0);
        let parse_nibble = |c: char| c.to_digit(16).map(|n| (n as f64) * 17.0 / 255.0);
        match hex.len() {
            3 => {
                let chs: Vec<char> = hex.chars().collect();
                let r = parse_nibble(chs[0])?;
                let g = parse_nibble(chs[1])?;
                let b = parse_nibble(chs[2])?;
                return Some((fmt_float(r), fmt_float(g), fmt_float(b), "1.0".to_string()));
            }
            4 => {
                // #RGBA shorthand — each nibble doubled, 4 channels.
                let chs: Vec<char> = hex.chars().collect();
                let r = parse_nibble(chs[0])?;
                let g = parse_nibble(chs[1])?;
                let b = parse_nibble(chs[2])?;
                let a = parse_nibble(chs[3])?;
                return Some((fmt_float(r), fmt_float(g), fmt_float(b), fmt_float(a)));
            }
            6 => {
                let r = parse_pair(&hex[0..2])?;
                let g = parse_pair(&hex[2..4])?;
                let b = parse_pair(&hex[4..6])?;
                return Some((fmt_float(r), fmt_float(g), fmt_float(b), "1.0".to_string()));
            }
            8 => {
                let r = parse_pair(&hex[0..2])?;
                let g = parse_pair(&hex[2..4])?;
                let b = parse_pair(&hex[4..6])?;
                let a = parse_pair(&hex[6..8])?;
                return Some((fmt_float(r), fmt_float(g), fmt_float(b), fmt_float(a)));
            }
            _ => {}
        }
    }
    None
}

/// Format an f64 as an LLVM-IR-compatible literal (always at least one
/// digit after the decimal point).
fn fmt_float(x: f64) -> String {
    if x.fract() == 0.0 {
        format!("{:.1}", x)
    } else {
        format!("{}", x)
    }
}

/// Lower a color expression to 4 channel values, with a runtime
/// fallback for non-literal inputs (issue #185 Phase C step 7).
///
/// Tries `extract_perry_color` first — that handles compile-time hex
/// strings, named colors, and `{r, g, b, a}` object literals. If that
/// returns `None`, the value is a runtime expression (e.g.,
/// `backgroundColor: someStringVar`); we lower the value once, then
/// emit 4 `js_color_parse_channel` calls (one per channel) against
/// it. The runtime parses the string per call (slight redundancy)
/// but keeps the LLVM IR trivial — single function call per channel,
/// no stack-alloca-of-array machinery needed.
fn lower_color_with_runtime_fallback(
    ctx: &mut FnCtx<'_>,
    val: &Expr,
) -> Result<(String, String, String, String)> {
    if let Some(rgba) = extract_perry_color(ctx, val)? {
        return Ok(rgba);
    }
    // Runtime fallback: lower expression once, then 4 channel calls.
    let value = lower_expr(ctx, val)?;
    ctx.pending_declares.push((
        "js_color_parse_channel".to_string(),
        DOUBLE,
        vec![DOUBLE, I64],
    ));
    let r = ctx.block().call(
        DOUBLE,
        "js_color_parse_channel",
        &[(DOUBLE, &value), (I64, "0")],
    );
    let g = ctx.block().call(
        DOUBLE,
        "js_color_parse_channel",
        &[(DOUBLE, &value), (I64, "1")],
    );
    let b = ctx.block().call(
        DOUBLE,
        "js_color_parse_channel",
        &[(DOUBLE, &value), (I64, "2")],
    );
    let a = ctx.block().call(
        DOUBLE,
        "js_color_parse_channel",
        &[(DOUBLE, &value), (I64, "3")],
    );
    Ok((r, g, b, a))
}

/// Extract a per-side padding object `{top?, right?, bottom?, left?}`
/// into the 4 sides (defaulting missing sides to 0). Returns `None` if
/// not an object literal.
fn extract_padding_sides(
    ctx: &mut FnCtx<'_>,
    val: &Expr,
) -> Result<Option<(String, String, String, String)>> {
    let Some(props) = extract_options_fields(ctx, val) else {
        return Ok(None);
    };
    let mut top = "0.0".to_string();
    let mut right = "0.0".to_string();
    let mut bottom = "0.0".to_string();
    let mut left = "0.0".to_string();
    for (key, v) in &props {
        let lowered = lower_expr(ctx, v)?;
        match key.as_str() {
            "top" => top = lowered,
            "right" => right = lowered,
            "bottom" => bottom = lowered,
            "left" => left = lowered,
            _ => {}
        }
    }
    Ok(Some((top, right, bottom, left)))
}

/// Extract a shadow object `{color?, blur?, offsetX?, offsetY?}` into
/// the 7 args `widget_set_shadow` takes. Defaults: black 25% opacity,
/// blur 0, offset (0, 0). Returns `None` if not an object literal.
fn extract_shadow_obj(
    ctx: &mut FnCtx<'_>,
    val: &Expr,
) -> Result<Option<(String, String, String, String, String, String, String)>> {
    let Some(props) = extract_options_fields(ctx, val) else {
        return Ok(None);
    };
    let mut cr = "0.0".to_string();
    let mut cg = "0.0".to_string();
    let mut cb = "0.0".to_string();
    let mut ca = "0.25".to_string();
    let mut blur = "0.0".to_string();
    let mut dx = "0.0".to_string();
    let mut dy = "0.0".to_string();
    for (key, v) in &props {
        match key.as_str() {
            "color" => {
                if let Some((r, g, b, a)) = extract_perry_color(ctx, v)? {
                    cr = r;
                    cg = g;
                    cb = b;
                    ca = a;
                }
            }
            "blur" => blur = lower_expr(ctx, v)?,
            "offsetX" => dx = lower_expr(ctx, v)?,
            "offsetY" => dy = lower_expr(ctx, v)?,
            _ => {}
        }
    }
    Ok(Some((cr, cg, cb, ca, blur, dx, dy)))
}

/// Phase C step 6: extract a `{ angle, stops: [c1, c2, ...] }` gradient
/// object into `(angle, color1_rgba, color2_rgba)`. Runtime FFI is
/// 2-color only; extra stops are ignored. Missing stops default to
/// fully transparent black so the resulting gradient renders cleanly.
fn extract_gradient_obj(
    ctx: &mut FnCtx<'_>,
    val: &Expr,
) -> Result<
    Option<(
        String,
        (String, String, String, String),
        (String, String, String, String),
    )>,
> {
    let Some(props) = extract_options_fields(ctx, val) else {
        return Ok(None);
    };
    let mut angle = "0.0".to_string();
    let transparent = (
        "0.0".to_string(),
        "0.0".to_string(),
        "0.0".to_string(),
        "0.0".to_string(),
    );
    let mut c1 = transparent.clone();
    let mut c2 = transparent;
    for (key, v) in &props {
        match key.as_str() {
            "angle" => angle = lower_expr(ctx, v)?,
            "stops" => {
                if let Expr::Array(elems) = v {
                    if let Some(first) = elems.first() {
                        if let Some(rgba) = extract_perry_color(ctx, first)? {
                            c1 = rgba;
                        }
                    }
                    if let Some(second) = elems.get(1) {
                        if let Some(rgba) = extract_perry_color(ctx, second)? {
                            c2 = rgba;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(Some((angle, c1, c2)))
}
