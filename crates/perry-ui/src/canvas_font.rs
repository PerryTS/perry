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
/// Only `px` and `pt` sizes are accepted.
pub fn parse_canvas_font(spec: &str) -> Option<CanvasFont> {
    let spec = spec.trim();
    let mut font = CanvasFont {
        families: Vec::new(),
        ..CanvasFont::default()
    };
    let mut rest = spec;
    loop {
        let (word, tail) = rest.split_once(char::is_whitespace)?;
        let lower = word.to_ascii_lowercase();
        if let Some(size) = parse_size(&lower) {
            font.size = size;
            rest = tail.trim_start();
            break;
        }
        match lower.as_str() {
            "normal" | "small-caps" => {}
            "italic" | "oblique" => font.italic = true,
            "bold" | "bolder" => font.bold = true,
            "lighter" => font.bold = false,
            w => font.bold = w.parse::<u16>().ok().filter(|n| (1..=1000).contains(n))? >= 600,
        }
        rest = tail.trim_start();
    }
    for family in rest.split(',') {
        let family = family.trim().trim_matches(|c| c == '"' || c == '\'');
        if family.is_empty() {
            return None;
        }
        font.families.push(family.to_string());
    }
    (!font.families.is_empty()).then_some(font)
}

fn parse_size(word: &str) -> Option<f64> {
    let word = word.split('/').next()?;
    let (num, scale) = if let Some(n) = word.strip_suffix("px") {
        (n, 1.0)
    } else {
        (word.strip_suffix("pt")?, 4.0 / 3.0)
    };
    let size = num.parse::<f64>().ok()? * scale;
    (size.is_finite() && size > 0.0).then_some(size)
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
        ] {
            assert_eq!(parse_canvas_font(bad), None, "{bad:?}");
        }
    }
}
