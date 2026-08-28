# Styling

Perry widgets accept an inline `style: { ... }` object that maps to each
platform's native styling APIs. The same shape works on every Widget
constructor — `Button`, `Text`, `Toggle`, `Slider`, `VStack`/`HStack`,
and friends — so cross-platform styling code stays the same regardless
of target.

## Inline style — recommended

Pass a `StyleProps` object as the trailing argument to any widget
constructor. Codegen destructures the literal at HIR time into a
sequence of native setter calls, so the runtime shape is the same as
hand-writing the imperative pattern below — but the source is much
shorter:

```typescript
{{#include ../../examples/ui/styling/button_inline_style.ts:button-inline-full}}
```

The `style` arg is optional; widgets without it look identical to
calls before this API existed. See
[`docs/examples/ui/styling/button_inline_style.ts`](../../examples/ui/styling/button_inline_style.ts)
for the full file.

### What `style` accepts

| Prop | Type | Maps to |
|---|---|---|
| `backgroundColor` | string \| PerryColor | `widgetSetBackgroundColor` |
| `color` | string \| PerryColor | `textSetColor` / `buttonSetTextColor` |
| `borderColor` | string \| PerryColor | `widgetSetBorderColor` |
| `borderWidth` | number | `widgetSetBorderWidth` |
| `borderRadius` | number | `setCornerRadius` |
| `padding` | number \| `{ top, right, bottom, left }` | `widgetSetEdgeInsets` |
| `opacity` | number (0..=1) | `widgetSetOpacity` |
| `shadow` | `{ color, blur, offsetX, offsetY }` | `widgetSetShadow` |
| `textDecoration` | `"none" \| "underline" \| "strikethrough"` | `textSetDecoration` |
| `gradient` | `{ angle, stops: [c1, c2] }` | `widgetSetBackgroundGradient` |
| `fontSize`, `fontWeight`, `fontFamily` | number / string | `textSetFont*` |
| `tooltip` | string | `widgetSetTooltip` |
| `hidden` | boolean | `widgetSetHidden` |
| `enabled` | boolean | `widgetSetEnabled` |

### Color values

Color props accept four interchangeable shapes:

```typescript,no-test
backgroundColor: "#3B82F6"                                   // hex 6/8
backgroundColor: "#3B82F6FF"                                 // hex with alpha
backgroundColor: "blue"                                      // named color
backgroundColor: { r: 0.231, g: 0.510, b: 0.965, a: 1.0 }   // PerryColor object
backgroundColor: themeColor                                  // runtime variable
```

Named colors: `white`, `black`, `red`, `green`, `blue`, `yellow`,
`cyan`, `magenta`, `gray` / `grey`, `transparent`. Hex forms supported:
`#RGB`, `#RGBA`, `#RRGGBB`, `#RRGGBBAA`.

Literals (the first four forms) compile-time-fold into 4 baked-in float
arguments — zero runtime cost. Runtime variables resolve through
`js_color_parse_channel` (a small CSS color parser in `perry-runtime`)
so `backgroundColor: someStringVar` works the same as the literal form.

### Padding shapes

A single number applies to all four sides; an object picks per-side:

```typescript,no-test
padding: 12                                       // all four sides 12
padding: { top: 8, right: 16, bottom: 8, left: 16 }  // per-side
```

Missing sides default to 0.

### Container styling

`VStack` and `HStack` accept `style` after the children array:

```typescript
{{#include ../../examples/ui/styling/stack_inline_style.ts:stack-inline-full}}
```

Both shapes work — `VStack(children, style?)` and `VStack(spacing, children, style?)`.

## Coming from CSS

If you're coming from web, the conceptual mapping is:

| CSS | Perry inline style |
|-----|-------|
| `display: flex; flex-direction: column` | `VStack(spacing, [...])` |
| `display: flex; flex-direction: row` | `HStack(spacing, [...])` |
| `width: 100%` | `widgetMatchParentWidth(widget)` |
| `padding: 10px 20px` | `padding: { top: 10, right: 20, bottom: 10, left: 20 }` |
| `gap: 16px` | `VStack(16, [...])` — first argument is the gap |
| CSS variables / design tokens | [`perry-styling`](theming.md) package |
| `opacity: 0.5` | `opacity: 0.5` |
| `border-radius: 8px` | `borderRadius: 8` |
| `background: #3B82F6` | `backgroundColor: "#3B82F6"` |
| `box-shadow: 0 4px 12px rgba(0,0,0,0.25)` | `shadow: { color: "#0004", blur: 12, offsetY: 4 }` |
| `text-decoration: underline` | `textDecoration: "underline"` |

See [Layout](layout.md) for full details on alignment, distribution, overlays, and split views.

## Imperative API (underlying)

The inline `style` object lowers to the same FFI calls as Perry's
imperative free-function setters: `widgetSet*`, `textSet*`,
`buttonSet*`. They take the widget handle as the first argument and
remain available for cases where you want fine-grained control or
need to mutate styles after creation. Colors here are RGBA floats in
`[0.0, 1.0]` (divide each hex byte by 255 — `0xFF3B30` →
`(1.0, 0.231, 0.188, 1.0)`).

Every snippet below is excerpted from
[`docs/examples/ui/styling/snippets.ts`](../../examples/ui/styling/snippets.ts),
which CI compiles and runs on every PR — so the API drawn here is always the
API the compiler accepts.

```typescript
{{#include ../../examples/ui/styling/snippets.ts:imports}}
```

### Colors

```typescript
{{#include ../../examples/ui/styling/snippets.ts:colors}}
```

### Fonts

```typescript
{{#include ../../examples/ui/styling/snippets.ts:fonts}}
```

Use `"monospaced"` for the system monospaced font.

### Corner Radius

```typescript
{{#include ../../examples/ui/styling/snippets.ts:corner-radius}}
```

### Borders

```typescript
{{#include ../../examples/ui/styling/snippets.ts:borders}}
```

### Padding and Insets

```typescript
{{#include ../../examples/ui/styling/snippets.ts:padding}}
```

### Sizing

```typescript
{{#include ../../examples/ui/styling/snippets.ts:sizing}}
```

### Opacity

```typescript
{{#include ../../examples/ui/styling/snippets.ts:opacity}}
```

### Background Gradient

```typescript
{{#include ../../examples/ui/styling/snippets.ts:gradient}}
```

### Control Size

```typescript
{{#include ../../examples/ui/styling/snippets.ts:control-size}}
```

> **macOS**: Maps to `NSControl.ControlSize`. Other platforms may interpret differently.

### Tooltips

```typescript
{{#include ../../examples/ui/styling/snippets.ts:tooltip}}
```

> **macOS/Windows/Linux**: Native tooltips. **iOS/Android**: No tooltip support. **Web**: HTML `title` attribute.

### Enabled/Disabled

```typescript
{{#include ../../examples/ui/styling/snippets.ts:enabled}}
```

### Complete Imperative Example

```typescript
{{#include ../../examples/ui/styling/counter_card.ts}}
```

### Composing Styles (imperative helper functions)

Reduce repetition by creating helper functions:

```typescript
{{#include ../../examples/ui/styling/snippets.ts:card-helper}}
```

For larger apps, use the `perry-styling` package to define design tokens in JSON and generate a typed theme file. See [Theming](theming.md) for the full workflow.

## Platform support

Per-prop, per-platform support is tracked in the
[styling matrix](styling-matrix.md) — auto-generated from
`crates/perry-ui/src/styling_matrix.rs` and CI-checked against each
backend's `lib.rs` exports on every PR.

The generated summary is the source of truth. It currently reports all 47
tracked properties wired, with zero stubs or missing exports, on macOS, iOS,
tvOS, visionOS, watchOS, Android, GTK4, Windows, and Web. The GTK4 and Windows
closure work is recorded in closed issues
[#202](https://github.com/PerryTS/perry/issues/202) and
[#210](https://github.com/PerryTS/perry/issues/210). Do not copy the counts
into another hand-maintained table; consult the matrix so the documentation
cannot drift from the backend inventory.

## Next Steps

- [Widgets](widgets.md) — All available widgets
- [Layout](layout.md) — Layout containers
- [Animation](animation.md) — Animate style changes
- [Theming](theming.md) — Design tokens via the `perry-styling` package
