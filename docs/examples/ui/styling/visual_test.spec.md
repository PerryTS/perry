# Visual styling test — verification spec

Companion to `visual_test.ts`. This document describes every cell in
the test app along with its **expected visual signature** so a tester
(human or LLM) can mechanically compare a screenshot against
expectations and flag regressions cell-by-cell.

## How to use

1. Compile + run the test on the platform you want to verify:
   ```bash
   ./target/release/perry compile docs/examples/ui/styling/visual_test.ts -o /tmp/styling_test
   ./tmp/styling_test
   ```
2. Capture the window (any screenshot tool, or via geisterhand's
   `/screenshot` endpoint where wired).
3. Walk this spec top-to-bottom. For each cell, confirm the screenshot
   matches the visible signature. Anything that doesn't match → file
   an issue against the underlying styling matrix row.

For LLM-assisted verification, send the screenshot + this file to
the model and ask "for each row, list any cells that don't match
their expected visible signature."

## Window meta

- **Title bar:** "Perry Styling Visual Test"
- **Dimensions:** 880 × 940 (resizable on platforms that support it)
- **Header:** large bold black "Perry Styling Visual Test" (20pt),
  followed by a small gray subtitle, then a horizontal divider line.
- **Body:** vertical stack of 13 numbered sections, each with a small
  gray label above and a horizontal row of widgets below.

## Section-by-section expectations

### 1. Colors

Five horizontally-arranged labels, all with white text, padding 8,
border-radius 4. Every cell should be a clearly-distinct solid color:

| # | Text | Expected background | Visible signature |
|---|------|---|---|
| 1 | `hex` | pure red `#FF0000` | white "hex" on FULL red box |
| 2 | `named` | pure blue `#0000FF` | white "named" on FULL blue box |
| 3 | `object` | medium green `(0, 0.6, 0, 1)` | white "object" on green box, slightly darker than pure green |
| 4 | `alpha` | red @ 50% alpha — composited over window background | white "alpha" on PINK / muted-red box (alpha shows through) |
| 5 | `runtime` | `#3B82F6` from a `themeBlue` variable | white "runtime" on bright cobalt-blue box (Phase C step 7 path) |

If cell 5 renders as anything other than the same blue as cell 2 of
gradient row, the runtime parseColor path is broken — cells 1-4 use
compile-time, cell 5 uses runtime, both should produce identical
visual results for the same color value.

### 2. Borders + corners

Four labels with transparent background + visible border, varying
width and corner-radius:

| # | Text | Border | Radius | Visible signature |
|---|------|---|---|---|
| 1 | `1px-4r` | thin black | slightly rounded | thin black outline, gentle corners |
| 2 | `3px-12r` | medium blue | well-rounded | thicker blue outline, pronounced rounding |
| 3 | `heavy` | thick red | square | bold red outline, sharp 90° corners |
| 4 | `pill` | thin gray | very rounded | nearly pill-shaped, hairline outline |

All four borders should render on every backend. Windows paints the stored
border state through its child-window paint subclass; GTK4 uses CSS.

### 3. Padding

Two labels with `#EEE` light-gray background:

| # | Text | Padding | Visible signature |
|---|------|---|---|
| 1 | `p:12` | uniform 12px | text centered with even gap on all sides |
| 2 | `p:4-24` | top:4 right:24 bottom:4 left:24 | text vertically tight, wide horizontal margins |

Cell 2's box should be visibly WIDER than cell 1 (more horizontal
padding) but SHORTER (less vertical padding).

### 4. Shadow

Three light-gray labels with rounded corners + black/colored shadows.
The shadow visibility test — if no shadow is visible, the platform's
shadow path isn't wired.

> **Note on bg color choice:** these cells use light gray `#F3F4F6` rather than
> pure white intentionally. CALayer's drop shadow is visually invisible when
> the card is the same color as the window background, even when the FFI is
> correctly applied — the bug originally observed before the macOS
> `setMasksToBounds: false` fix landed.

| # | Text | Shadow color | Blur | Offset | Visible signature |
|---|------|---|---|---|---|
| 1 | `soft` | black @ 100% (full alpha) | 12 | (0, 4) | soft dark blur below light-gray box |
| 2 | `hard` | black | 0 | (4, 4) | sharp dark offset to bottom-right of light-gray box |
| 3 | `blue` | blue | 16 | (0, 6) | soft blue glow below light-gray box |

**Android:** shadow direction may differ from
iOS/macOS (Android's elevation derives direction from device-level
light source, not the offset; the offset is intentionally ignored —
same shadow color/blur should appear under all 3 cells).

### 5. Gradient

Three labels with linear gradients at different angles. White text:

| # | Text | Gradient | Angle | Visible signature |
|---|------|---|---|---|
| 1 | `→` | blue→purple | 90° (left to right) | left edge blue, right edge purple, smooth horizontal blend |
| 2 | `↓` | red→orange | 180° (top to bottom) | top edge red, bottom edge orange, smooth vertical blend |
| 3 | `↘` | green→cyan | 135° (diagonal) | top-left green, bottom-right cyan, diagonal blend |

### 6. Opacity

Four `#3B82F6` blue labels at decreasing opacity:

| # | Text | Opacity | Visible signature |
|---|------|---|---|
| 1 | `100` | 1.0 | full bright cobalt blue |
| 2 | `75` | 0.75 | slightly faded, still clearly blue |
| 3 | `50` | 0.5 | half-transparent, washed-out blue |
| 4 | `25` | 0.25 | very faint, almost ghosted |

**GTK4 / Windows:** opacity is wired on both platforms (`Widget::set_opacity`
on GTK4 and layered-window alpha on Windows). Cells 2-4 should be visibly
fainter on each.

### 7. Typography

Seven text labels in a row, all black text, demonstrating font axes:

| # | Text | Style | Visible signature |
|---|------|---|---|
| 1 | `normal` | regular 14pt | baseline reference |
| 2 | `bold` | weight 700 | visibly heavier strokes than cell 1 |
| 3 | `under` | underline | "under" with horizontal line below |
| 4 | `strike` | strikethrough | "strike" with horizontal line through middle |
| 5 | `12pt` | smaller | visibly smaller than cell 1 |
| 6 | `18pt` | larger | visibly larger than cell 1 |
| 7 | `mono` | monospaced | distinctive monospace glyph shapes |

**Windows:** text decoration is applied through the Text widget's Win32 font;
cells 3 and 4 should show the underline and strikeout.

### 8. Buttons

Four buttons in a row:

| # | Label | Style | Visible signature |
|---|------|---|---|
| 1 | `Default` | none | platform-native button (rounded rect on macOS, square on Windows, etc.) |
| 2 | `Styled` | blue bg, white text, 6r radius, 8 padding | flat blue rectangle, white "Styled" |
| 3 | `Outlined` | 2px blue border, 6r, 8 padding | transparent rectangle with blue outline |
| 4 | `Disabled` | enabled: false | grayed out, won't respond to clicks |

### 9. Inputs

Two text input controls. Verify both render at all + accept focus:

| # | Type | Placeholder | Visible signature |
|---|------|---|---|
| 1 | `TextField` | "Type here…" | single-line input box with gray placeholder |
| 2 | `SecureField` | "Password" | single-line input that masks typed chars |

### 10. Controls

Three control widgets:

| # | Type | Setup | Visible signature |
|---|------|---|---|
| 1 | `Toggle` | label "Enable" | iOS-style switch (on macOS: checkbox-style; iOS: pill switch) |
| 2 | `Slider` | min 0, max 100 | horizontal slider track with movable thumb |
| 3 | `ProgressView` | indeterminate | spinning indicator OR horizontal bar showing progress |

### 11. Image (SF Symbols)

Three symbol icons. SF Symbols only render on Apple platforms; on
GTK4 / Android / Web you'll get a fallback or empty space.

| # | Symbol | Visible signature |
|---|------|---|
| 1 | `star.fill` | filled 5-pointed star |
| 2 | `heart.fill` | filled heart shape |
| 3 | `circle.fill` | filled circle |

**Non-Apple platforms:** these may render as empty placeholders or
default fallback icons. That's expected; Perry's image-by-symbol is
SF-Symbols-specific.

### 12. States

Three labels demonstrating visibility flags:

| # | Text | State | Visible signature |
|---|------|---|---|
| 1 | `visible` | normal | green box with white "visible" |
| 2 | `hidden!` | hidden: true | **NOTHING — should NOT appear** |
| 3 | `opacity-0` | opacity: 0 | **NOTHING — should NOT appear** (or invisible blue box that takes layout space) |

If cells 2 or 3 are visible, the state-based hiding is broken on
that platform. Note: cell 3 (`opacity: 0`) may still occupy layout
space on some backends (the box is invisible but pushes siblings).
That's correct behavior — opacity-0 ≠ hidden.

### 13. Runtime colors

Two runtime-resolved color cells (Phase C step 7 path):

| # | Text | Color source | Visible signature |
|---|------|---|---|
| 1 | `var-hex` | `themeBlue` = `"#3B82F6"` | white text on cobalt-blue box (matches color row cell 5) |
| 2 | `var-bad` | `themePurple` = `"rebeccapurple"` (NOT a Perry-supported named color) | white text on box with **fallback color** — black with alpha 1.0 because `parse_css_color` returns None for unknown named colors and the runtime returns 0.0 for r/g/b channels and 1.0 for alpha (see `crates/perry-runtime/src/color_parse.rs::js_color_parse_channel`) |

If cell 2 renders as the actual purple color of `rebeccapurple`,
that means the runtime path supports MORE named colors than the spec
documents (good — file a docs update). If cell 2 renders as
something else entirely, the runtime path is broken.

## Per-platform expected status

Platform → expected sections, based on the current generated styling matrix:

| Platform | Sections expected fully ✓ | Known platform differences |
|----------|--------------------------|-----------------------------------|
| **macOS / iOS / tvOS / visionOS / watchOS** | 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13 | none |
| **Android** | 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 12, 13 | 11 (no SF Symbols), 4 (shadow direction may differ — offset ignored on Android by design) |
| **GTK4** | 1, 2, 3, 5, 6, 7, 8, 9, 10, 12, 13 | 4 (exact shadow appearance can depend on GTK theme), 11 (no SF Symbols) |
| **Windows** | 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 12, 13 | 11 (no SF Symbols) |
| **Web** | 1, 2, 3, 4, 5, 6, 7, 9, 10, 12, 13 | 11 (no SF Symbols, fallback may show empty boxes) |

## When something fails

If a cell doesn't match its visible signature on a platform where
the matrix says it should:

1. Confirm the matrix entry: `./target/debug/styling-matrix --gen` then
   inspect `docs/src/ui/styling-matrix.md` for the relevant prop row.
2. If matrix says `Wired` but visual fails → file a new issue
   referencing this spec section + the platform.
3. If the matrix says `Stub` or `Missing`, document the limitation beside the
   affected cell and link its active tracker. The current matrix has no such
   styling cells.
