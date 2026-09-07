//! Geometry helpers shared by the UIKit backends (iOS, tvOS, visionOS).

/// Shrink a rect `(x, y, w, h)` by insets `(top, left, bottom, right)`, clamping
/// width and height at 0 so the rect never inverts. UIKit is origin-top-left, so
/// the top inset moves the origin down; an AppKit cell draws origin-bottom-left
/// and must not use this.
pub fn shrink_rect_uikit(
    insets: (f64, f64, f64, f64),
    rect: (f64, f64, f64, f64),
) -> (f64, f64, f64, f64) {
    let (top, left, bottom, right) = insets;
    let (x, y, w, h) = rect;
    (
        x + left,
        y + top,
        (w - left - right).max(0.0),
        (h - top - bottom).max(0.0),
    )
}

#[cfg(test)]
mod tests {
    use super::shrink_rect_uikit;

    #[test]
    fn shrink_rect_uikit_applies_and_clamps_insets() {
        // (label, insets (top, left, bottom, right), in (x, y, w, h), expected (x, y, w, h))
        // UIKit is top-left, so left moves origin.x and top moves origin.y.
        let cases = [
            (
                "all sides",
                (10.0, 20.0, 30.0, 40.0),
                (0.0, 0.0, 200.0, 100.0),
                (20.0, 10.0, 140.0, 60.0),
            ),
            (
                "zero insets are identity",
                (0.0, 0.0, 0.0, 0.0),
                (5.0, 6.0, 30.0, 40.0),
                (5.0, 6.0, 30.0, 40.0),
            ),
            (
                "insets larger than the rect clamp size to zero",
                (100.0, 100.0, 100.0, 100.0),
                (0.0, 0.0, 50.0, 50.0),
                (100.0, 100.0, 0.0, 0.0),
            ),
        ];
        for (label, insets, rect, expected) in cases {
            assert_eq!(shrink_rect_uikit(insets, rect), expected, "case: {label}");
        }
    }
}
