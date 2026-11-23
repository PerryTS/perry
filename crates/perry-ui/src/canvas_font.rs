//! The CSS `font` shorthand that `canvas.setFont` takes, parsed once for every
//! native backend. Anything it cannot read keeps the previous font, as HTML
//! canvas ignores an invalid `font` assignment.

/// A canvas font. `size` is in CSS pixels; `families` keeps the CSS order and
/// may name generics (`sans-serif`, `serif`, `monospace`).
#[derive(Clone, Debug, PartialEq)]
pub struct CanvasFont {
    pub size: f64,
    pub bold: bool,
    pub italic: bool,
    pub families: Vec<String>,
}

impl Default for CanvasFont {
    /// HTML canvas starts at `10px sans-serif`.
    fn default() -> Self {
        Self {
            size: 10.0,
            bold: false,
            italic: false,
            families: vec!["sans-serif".into()],
        }
    }
}

/// Parses `[style] [variant] [weight] size[/line-height] family[, family…]`.
/// Only `px` and `pt` sizes are accepted; a line height is validated and
/// dropped, since `fillText` draws a single line.
pub fn parse_canvas_font(spec: &str) -> Option<CanvasFont> {
    let mut font = CanvasFont {
        families: Vec::new(),
        ..CanvasFont::default()
    };
    let mut rest = spec.trim();
    // Style, variant and weight may each appear once; `normal` sets none of them.
    let (mut style, mut variant, mut weight, mut prefixes) = (false, false, false, 0);
    loop {
        let end = rest
            .find(|c: char| c.is_whitespace() || c == '/')
            .unwrap_or(rest.len());
        let (word, tail) = rest.split_at(end);
        let lower = word.to_ascii_lowercase();
        if let Some(size) = parse_size(&lower) {
            font.size = size;
            rest = tail.trim_start();
            break;
        }
        prefixes += 1;
        let seen = match lower.as_str() {
            "normal" => false,
            "small-caps" => std::mem::replace(&mut variant, true),
            "italic" | "oblique" => {
                font.italic = true;
                std::mem::replace(&mut style, true)
            }
            w => {
                font.bold = match w {
                    "bold" | "bolder" => true,
                    "lighter" => false,
                    n => {
                        n.parse::<f64>()
                            .ok()
                            .filter(|n| (1.0..=1000.0).contains(n))?
                            >= 600.0
                    }
                };
                std::mem::replace(&mut weight, true)
            }
        };
        if seen || prefixes > 3 {
            return None;
        }
        rest = tail.trim_start();
        if rest.is_empty() {
            return None;
        }
    }
    if let Some(after) = rest.strip_prefix('/') {
        let after = after.trim_start();
        let end = after.find(char::is_whitespace).unwrap_or(after.len());
        if !is_line_height(&after[..end].to_ascii_lowercase()) {
            return None;
        }
        rest = after[end..].trim_start();
    }
    for family in rest.split(',') {
        let family = family.trim();
        let quoted = family.len() >= 2
            && (family.starts_with('"') && family.ends_with('"')
                || family.starts_with('\'') && family.ends_with('\''));
        let name = if quoted {
            &family[1..family.len() - 1]
        } else {
            family
        };
        // CSS-wide keywords are not family names unless quoted; control
        // characters (NUL among them) cannot reach a platform font API.
        if name.is_empty()
            || name.chars().any(char::is_control)
            || !quoted
                && (CSS_WIDE_KEYWORDS.contains(&name.to_ascii_lowercase().as_str())
                    || !name.split_whitespace().all(is_ident))
        {
            return None;
        }
        font.families.push(name.to_string());
    }
    (!font.families.is_empty()).then_some(font)
}

const CSS_WIDE_KEYWORDS: &[&str] = &[
    "inherit",
    "initial",
    "unset",
    "revert",
    "revert-layer",
    "default",
];

fn parse_size(word: &str) -> Option<f64> {
    let (num, scale) = if let Some(n) = word.strip_suffix("px") {
        (n, 1.0)
    } else {
        (word.strip_suffix("pt")?, 4.0 / 3.0)
    };
    let size = num.parse::<f64>().ok()? * scale;
    (size.is_finite() && size > 0.0).then_some(size)
}

/// `normal`, or a non-negative number, percentage or CSS length.
fn is_line_height(word: &str) -> bool {
    const UNITS: &[&str] = &[
        "", "%", "px", "pt", "pc", "cm", "mm", "q", "in", "em", "rem", "ex", "rex", "ch", "rch",
        "cap", "rcap", "ic", "ric", "lh", "rlh", "vw", "vh", "vi", "vb", "vmin", "vmax",
    ];
    // Any unit whose removal leaves a whole CSS number, so `1e0` and `1rem` both parse.
    word == "normal"
        || UNITS.iter().any(|unit| {
            word.strip_suffix(unit)
                .and_then(|num| num.parse::<f64>().ok())
                .is_some_and(|n| n.is_finite() && n >= 0.0)
        })
}

/// An unquoted CSS identifier: no leading digit (or `-` + digit), and only
/// letters, digits, `-`, `_` or non-ASCII characters.
fn is_ident(word: &str) -> bool {
    let body = word.strip_prefix('-').unwrap_or(word);
    body.chars().next().is_some_and(|c| !c.is_ascii_digit())
        && word
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || !c.is_ascii())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font(size: f64, bold: bool, italic: bool, families: &[&str]) -> CanvasFont {
        CanvasFont {
            size,
            bold,
            italic,
            families: families.iter().map(|f| f.to_string()).collect(),
        }
    }

    #[test]
    fn parses_the_shorthand_forms_canvas_code_uses() {
        assert_eq!(
            parse_canvas_font("16px sans-serif"),
            Some(font(16.0, false, false, &["sans-serif"]))
        );
        assert_eq!(
            parse_canvas_font("italic bold 12px/1.5 \"Helvetica Neue\", Arial, sans-serif"),
            Some(font(
                12.0,
                true,
                true,
                &["Helvetica Neue", "Arial", "sans-serif"]
            ))
        );
        assert_eq!(
            parse_canvas_font("600 9pt Menlo"),
            Some(font(12.0, true, false, &["Menlo"]))
        );
        assert_eq!(
            parse_canvas_font("300 14px serif"),
            Some(font(14.0, false, false, &["serif"]))
        );
        assert_eq!(
            parse_canvas_font("bold 16px/1rem Times New Roman"),
            Some(font(16.0, true, false, &["Times New Roman"]))
        );
        assert_eq!(
            parse_canvas_font("normal italic small-caps 700 8px serif"),
            None,
            "four prefixes"
        );
        assert_eq!(
            parse_canvas_font("700.5 20px/1e0 serif"),
            Some(font(20.0, true, false, &["serif"]))
        );
        assert_eq!(
            parse_canvas_font("italic small-caps 700 8px serif"),
            Some(font(8.0, true, true, &["serif"]))
        );
        assert_eq!(
            parse_canvas_font("16px / 1.5 Menlo"),
            Some(font(16.0, false, false, &["Menlo"]))
        );
        assert_eq!(
            parse_canvas_font("bold 16px/normal 'inherit', serif"),
            Some(font(16.0, true, false, &["inherit", "serif"]))
        );
    }

    #[test]
    fn rejects_what_html_canvas_ignores() {
        for bad in [
            "",
            "sans-serif",
            "16 sans-serif",
            "16px",
            "16px ,",
            "0px serif",
            "-4px serif",
            "heavy 12px serif",
            "2em serif",
            "16px/nonsense Menlo",
            "16px / Menlo",
            "12px inherit",
            "12px serif, initial",
            "12px Ex\u{0}ample",
            "italic italic 20px sans-serif",
            "bold 600 20px serif",
            "normal normal normal normal 20px serif",
            "12px Arial;",
            "12px 3D Font",
            "16px/1r serif",
            "16px/-1 serif",
            "16px/inf serif",
            "1000.5 16px serif",
        ] {
            assert_eq!(parse_canvas_font(bad), None, "{bad:?}");
        }
    }
}
