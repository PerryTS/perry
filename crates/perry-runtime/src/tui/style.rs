//! Style props for Box widgets — the subset of flexbox we expose.
//!
//! Phase 3 (#358) shipped the v1.0 surface: flexDirection,
//! justifyContent, alignItems, gap, uniform `padding`, explicit
//! integer-cell `width`/`height`, and `flexGrow`. Phase 3.5 (#405)
//! adds `padding: { top, right, bottom, left }` per-side, `flexShrink`,
//! `flexBasis`, and percentage units (`width: "50%"`).
//!
//! User-facing TS shape:
//!
//! ```typescript
//! Box({
//!   flexDirection: "row" | "column",
//!   justifyContent: "start" | "center" | "end" | "space-between" | "space-around",
//!   alignItems: "start" | "center" | "end" | "stretch",
//!   gap: number,
//!   padding: number | { top, right, bottom, left },
//!   width: number | string,    // "50%" → percentage of parent
//!   height: number | string,
//!   flexGrow: number,
//!   flexShrink: number,
//!   flexBasis: number | string,
//! }, [child1, child2])
//! ```

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FlexDirection {
    Row,
    #[default]
    Column,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum JustifyContent {
    #[default]
    Start,
    Center,
    End,
    SpaceBetween,
    SpaceAround,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AlignItems {
    #[default]
    Start,
    Center,
    End,
    Stretch,
}

/// Per-side cell counts. Used for padding (and, in the future, margin /
/// border).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Edges {
    pub top: u16,
    pub right: u16,
    pub bottom: u16,
    pub left: u16,
}

impl Edges {
    /// Uniform edges — all four sides equal.
    pub const fn all(n: u16) -> Self {
        Edges {
            top: n,
            right: n,
            bottom: n,
            left: n,
        }
    }
}

/// A length expressed in cells or as a percentage of the parent's
/// equivalent dimension. Percent stored as basis points (0..=10000)
/// so the type stays `Eq` (no f32 NaN edge cases) and the codegen
/// can pass it as an integer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Length {
    Cells(u16),
    /// Basis points. 5000 = 50%, 10000 = 100%.
    PercentBp(u16),
}

impl Length {
    /// Construct from a percentage in 0.0..=100.0 range. Out-of-range
    /// values are clamped.
    pub fn percent(pct: f32) -> Self {
        let bp = (pct.clamp(0.0, 100.0) * 100.0).round() as u16;
        Length::PercentBp(bp)
    }
}

/// A Box's style. Defaults match the v0.1 vertical-stack behavior so
/// existing code keeps working without supplying a style.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct BoxStyle {
    pub flex_direction: FlexDirection,
    pub justify_content: JustifyContent,
    pub align_items: AlignItems,
    /// Cells of space between adjacent children.
    pub gap: u16,
    /// Per-side padding in cells. Use `Edges::all(n)` for uniform.
    pub padding: Edges,
    /// Explicit width. `None` = auto (fill parent or content).
    pub width: Option<Length>,
    /// Explicit height.
    pub height: Option<Length>,
    /// CSS flex-grow factor. 0 = no grow (default); `Spacer()` sets
    /// this to 1 for "fill remaining space" behavior.
    pub flex_grow: u16,
    /// CSS flex-shrink factor. 1 = shrink at default rate, 0 = don't
    /// shrink. Default 0 — opt-in only since the v1 layout never
    /// shrunk children.
    pub flex_shrink: u16,
    /// CSS flex-basis. `None` = auto.
    pub flex_basis: Option<Length>,
}

/// Parse a flexDirection string into the enum. Unknown strings fall
/// back to Column (the default vertical stack).
pub fn parse_flex_direction(s: &str) -> FlexDirection {
    match s {
        "row" => FlexDirection::Row,
        _ => FlexDirection::Column,
    }
}

pub fn parse_justify_content(s: &str) -> JustifyContent {
    match s {
        "center" => JustifyContent::Center,
        "end" | "flex-end" => JustifyContent::End,
        "space-between" => JustifyContent::SpaceBetween,
        "space-around" => JustifyContent::SpaceAround,
        _ => JustifyContent::Start,
    }
}

pub fn parse_align_items(s: &str) -> AlignItems {
    match s {
        "center" => AlignItems::Center,
        "end" | "flex-end" => AlignItems::End,
        "stretch" => AlignItems::Stretch,
        _ => AlignItems::Start,
    }
}

/// Parse a length string. Recognized shapes:
///   - `"50%"` → `Length::PercentBp(5000)`
///   - `"42"` / `"42cells"` → `Length::Cells(42)`
///   - empty / unparseable → `None`
pub fn parse_length(s: &str) -> Option<Length> {
    let s = s.trim();
    if let Some(rest) = s.strip_suffix('%') {
        let n: f32 = rest.trim().parse().ok()?;
        return Some(Length::percent(n));
    }
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    let n: u32 = digits.parse().ok()?;
    Some(Length::Cells(n.min(u16::MAX as u32) as u16))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flex_direction_parsing() {
        assert_eq!(parse_flex_direction("row"), FlexDirection::Row);
        assert_eq!(parse_flex_direction("column"), FlexDirection::Column);
        assert_eq!(parse_flex_direction(""), FlexDirection::Column);
        assert_eq!(parse_flex_direction("garbage"), FlexDirection::Column);
    }

    #[test]
    fn justify_content_parsing() {
        assert_eq!(parse_justify_content("start"), JustifyContent::Start);
        assert_eq!(parse_justify_content("center"), JustifyContent::Center);
        assert_eq!(parse_justify_content("end"), JustifyContent::End);
        assert_eq!(parse_justify_content("flex-end"), JustifyContent::End);
        assert_eq!(
            parse_justify_content("space-between"),
            JustifyContent::SpaceBetween
        );
        assert_eq!(parse_justify_content("garbage"), JustifyContent::Start);
    }

    #[test]
    fn align_items_parsing() {
        assert_eq!(parse_align_items("start"), AlignItems::Start);
        assert_eq!(parse_align_items("center"), AlignItems::Center);
        assert_eq!(parse_align_items("stretch"), AlignItems::Stretch);
    }

    #[test]
    fn default_box_style_is_column_zero() {
        let s = BoxStyle::default();
        assert_eq!(s.flex_direction, FlexDirection::Column);
        assert_eq!(s.gap, 0);
        assert_eq!(s.padding, Edges::default());
        assert_eq!(s.width, None);
        assert_eq!(s.flex_shrink, 0);
        assert_eq!(s.flex_basis, None);
    }

    #[test]
    fn edges_all_sets_every_side() {
        let e = Edges::all(3);
        assert_eq!(e.top, 3);
        assert_eq!(e.right, 3);
        assert_eq!(e.bottom, 3);
        assert_eq!(e.left, 3);
    }

    #[test]
    fn length_percent_clamps() {
        assert_eq!(Length::percent(50.0), Length::PercentBp(5000));
        assert_eq!(Length::percent(0.0), Length::PercentBp(0));
        assert_eq!(Length::percent(100.0), Length::PercentBp(10000));
        // Out of range — clamp.
        assert_eq!(Length::percent(150.0), Length::PercentBp(10000));
        assert_eq!(Length::percent(-5.0), Length::PercentBp(0));
    }

    #[test]
    fn parse_length_recognizes_percent() {
        assert_eq!(parse_length("50%"), Some(Length::PercentBp(5000)));
        assert_eq!(parse_length("100%"), Some(Length::PercentBp(10000)));
        assert_eq!(parse_length("33.5%"), Some(Length::PercentBp(3350)));
    }

    #[test]
    fn parse_length_recognizes_cells() {
        assert_eq!(parse_length("42"), Some(Length::Cells(42)));
        assert_eq!(parse_length("42cells"), Some(Length::Cells(42)));
        assert_eq!(parse_length(""), None);
        assert_eq!(parse_length("abc"), None);
    }
}
