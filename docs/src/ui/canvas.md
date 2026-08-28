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

### Text

```typescript
{{#include ../../examples/ui/canvas/snippets.ts:text}}
```

## Platform Notes

| Platform | Implementation | Status |
|----------|---------------|--------|
| Web | HTML5 Canvas | Wired |
| WASM | HTML5 Canvas via JS bridge | Wired |
| macOS | Core Graphics (CGContext) | Partial infrastructure; stateful API incomplete |
| iOS | Core Graphics (CGContext) | Partial infrastructure; stateful API incomplete |
| Linux | Cairo | Partial infrastructure; stateful API incomplete |
| Windows | GDI command buffer | Partial infrastructure; stateful API incomplete |
| Android | Canvas/Bitmap | Partial infrastructure; stateful API incomplete |

The native parity matrix currently marks `setFillColor`, `setStrokeColor`,
`setLineWidth`, `fillRect`, `strokeRect`, `arc`, `closePath`, `fill`,
stateful `stroke`, `setFont`, and `fillText` unsupported. `Canvas()` creation,
clear/begin/move/line primitives, the lower-level colored stroke and gradient
entries, and image drawing have native implementations, but that lower-level
surface does not make the HTML-style examples above fully functional.

## Next Steps

- [Widgets](widgets.md) — All available widgets
- [Animation](animation.md) — Animating widget properties
- [Styling](styling.md) — Widget styling
