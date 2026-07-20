# UI Overview

Perry's `perry/ui` module lets you build native desktop and mobile apps with declarative TypeScript. Your UI code compiles directly to platform-native widgets — AppKit on macOS, UIKit on iOS, GTK4 on Linux, Win32 on Windows, and DOM elements on the web.

## Quick Start

```typescript
{{#include ../../examples/ui/overview/quickstart.ts}}
```

```bash
perry app.ts -o app && ./app
```

## Mental Model

Perry's UI follows the same model as SwiftUI and Flutter: you compose native widgets using stack-based layout containers (`VStack`, `HStack`, `ZStack`), control alignment and distribution, and style widgets via free functions that take the widget handle as their first argument (`textSetColor(label, r, g, b, a)`, `widgetSetEdgeInsets(stack, ...)`, etc.). If you're coming from web development, the key shift is:

- **Layout** is controlled by stack alignment, distribution, and spacers — not CSS properties. See [Layout](layout.md).
- **Styling** is applied directly to widgets — not through stylesheets. See [Styling](styling.md).
- **Absolute positioning** uses overlays (`widgetAddOverlay` + `widgetSetOverlayFrame`) — not `position: absolute/relative`.
- **Design tokens** come from the `perry-styling` package. See [Theming](theming.md).

## App Lifecycle

Every Perry UI app starts with `App()`:

```typescript
{{#include ../../examples/ui/overview/snippets.ts:app-shell}}
```

`App({})` accepts a config object with the following properties:

| Property | Type | Description |
|----------|------|-------------|
| `title` | string | Window title |
| `width` | number | Initial window width |
| `height` | number | Initial window height |
| `body` | widget | Root widget |
| `icon` | string | App icon file path (optional) |
| `windowState` | string | Initial state: `"normal"`, `"maximized"`, `"fullscreen"` (optional) |
| `frameless` | boolean | Remove title bar (optional) |
| `level` | string | Window z-order: `"floating"`, `"statusBar"`, `"modal"` (optional) |
| `transparent` | boolean | Transparent background (optional) |
| `vibrancy` | string | Native blur material, e.g. `"sidebar"` (optional) |
| `activationPolicy` | string | `"regular"`, `"accessory"` (no dock icon), `"background"` (optional) |

See [Multi-Window](multi-window.md#app-window-properties) for full
documentation on window properties, including the native primitive each
field maps to per platform.

### Lifecycle Hooks

```typescript
{{#include ../../examples/ui/overview/snippets.ts:lifecycle}}
```

## Widget Tree

Perry UIs are built as a tree of widgets:

```typescript
{{#include ../../examples/ui/overview/snippets.ts:widget-tree}}
```

Widgets are created by calling their constructor functions. Layout containers (`VStack`, `HStack`, `ZStack`) accept a spacing value (in points) followed by an array of child widgets.

## Handle-Based Architecture

Under the hood, each widget is a handle — a small integer that references a native platform object. When you call `Text("hello")`, Perry creates a native `NSTextField` (macOS), `UILabel` (iOS), `GtkLabel` (Linux), or `<span>` (web) and returns a handle you can use to modify it.

```typescript
{{#include ../../examples/ui/overview/snippets.ts:handle-modify}}
```

## Imports

All UI functions are imported from `perry/ui`:

```typescript
{{#include ../../examples/ui/overview/imports.ts:imports}}
```

> [`Canvas`](canvas.md), [`CameraView`](camera.md), and the virtualized
> [`Table`](table.md) widget are wired through the LLVM codegen (closed via
> [#190](https://github.com/PerryTS/perry/issues/190),
> [#191](https://github.com/PerryTS/perry/issues/191), and
> [#192](https://github.com/PerryTS/perry/issues/192)). See each widget's page
> for the platform-support matrix.

## Platform Differences

The same code runs on all platforms, but the look and feel matches each platform's native style:

| Feature | macOS | iOS | Linux | Windows | Web |
|---------|-------|-----|-------|---------|-----|
| Buttons | NSButton | UIButton | GtkButton | HWND Button | `<button>` |
| Text | NSTextField | UILabel | GtkLabel | Static HWND | `<span>` |
| Layout | NSStackView | UIStackView | GtkBox | Manual layout | Flexbox |
| Menus | NSMenu | — | GMenu | HMENU | DOM |

Platform-specific behavior is noted on each widget's documentation page.

## Next Steps

- [Widgets](widgets.md) — All available widgets
- [Layout](layout.md) — Arranging widgets
- [State Management](state.md) — Reactive state and bindings
- [Styling](styling.md) — Colors, fonts, sizing
- [Events](events.md) — Click, hover, keyboard
