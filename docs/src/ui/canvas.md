# Canvas

The `Canvas` widget provides a 2D drawing surface for custom graphics.

> **Availability**: the `Canvas` handle and method-dispatch surface compile and
> link on every backend, which was the scope closed by
> [#190](https://github.com/PerryTS/perry/issues/190). Full stateful 2D
> rasterization is currently supported by the web backend only. Native
> backends have canvas creation plus lower-level path/gradient/image
> infrastructure, but the HTML-style state setters and drawing calls used in
> the examples below are still classified `Unsupported` in
> `crates/perry-ui-test` and are stubbed or incomplete. The snippets are
> compile-link verified by the doc-tests harness against
> [`docs/examples/ui/canvas/snippets.ts`](https://github.com/PerryTS/perry/blob/main/docs/examples/ui/canvas/snippets.ts);
> that proves API routing, not visible native pixels. See that file for the
> full standalone program.

The drawing API is **method-based** on the canvas handle (matching the FFI
shape — `perry_ui_canvas_set_fill_color(handle, r, g, b, a)` etc.). Colors
are RGBA floats in `[0.0, 1.0]`.

## Creating a Canvas

```typescript
{{#include ../../examples/ui/canvas/snippets.ts:create}}
```

`Canvas(width, height)` creates a canvas widget; subsequent draw operations
are method calls on the returned handle.

## Drawing Shapes

### Rectangles

```typescript
{{#include ../../examples/ui/canvas/snippets.ts:rectangles}}
```

### Lines

```typescript
{{#include ../../examples/ui/canvas/snippets.ts:lines}}
```

### Circles and Arcs

```typescript
{{#include ../../examples/ui/canvas/snippets.ts:arcs}}
```

### Gradients

```typescript
{{#include ../../examples/ui/canvas/snippets.ts:gradient}}
```

`fillGradient` fills the area between the current path and the bottom edge
with a linear gradient between two RGBA colors that spans the canvas;
`direction` `0` runs top to bottom and `1` left to right. The path is
closed straight down to the bottom edge, so the example above shades the area
under its line on every backend; a path with fewer than two points fills
nothing. Only `moveTo`/`lineTo` points shape the area, joined in order into one
outline: a later `moveTo` continues it rather than starting a separate area,
so draw one series per `beginPath`. The fill color set by `setFillColor` is
unchanged afterwards.

### Text

```typescript
{{#include ../../examples/ui/canvas/snippets.ts:text}}
```

## Platform Notes

| Platform | Implementation | Status |
|----------|---------------|--------|
| Web | HTML5 Canvas | Wired |
| WASM | HTML5 Canvas via JS bridge | Wired |
| macOS | Core Graphics (CGContext) | Wired |
| iOS, tvOS, visionOS | Core Graphics (CGContext) | Wired |
| Linux | Cairo | Wired |
| Windows | GDI command buffer | Wired |
| Android | Canvas/Bitmap | Wired |
| watchOS | — | Ignored (no-op) |

The stateful API (`setFillColor`, `setStrokeColor`, `setLineWidth`,
`fillRect`, `strokeRect`, `beginPath`, `moveTo`, `lineTo`, `arc`, `closePath`,
`fill` and `stroke`) follows HTML canvas: fill and stroke start black with a
line width of 1, `arc` takes radians and runs clockwise, and `fill` uses the
nonzero rule. watchOS ignores the whole API. Windows draws opaque, because GDI
has no alpha; macOS and Windows flatten arcs to short line segments.
`setFont` and `fillText` draw text natively on
macOS, iOS, tvOS, visionOS, Android, Linux and Windows, in the current fill
color with `y` on the alphabetic baseline; watchOS ignores them. Windows draws
text opaque, because GDI text has no alpha, and Linux uses only the first
family of the font list. `setFont` reads the CSS `font` shorthand with `px` or
`pt` sizes; any other spec leaves the font unchanged. `Canvas()` creation,
clear/begin/move/line primitives, the lower-level colored stroke and gradient
entries, and image drawing have native implementations.

## Next Steps

- [Widgets](widgets.md) — All available widgets
- [Animation](animation.md) — Animating widget properties
- [Styling](styling.md) — Widget styling
