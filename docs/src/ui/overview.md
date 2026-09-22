# UI Overview

Perry's `perry/ui` module lets you build native desktop and mobile apps with declarative TypeScript. Your UI code compiles directly to platform-native widgets — AppKit on macOS, UIKit on iOS, GTK4 on Linux, Win32 on Windows, and DOM elements on the web.

## Quick Start

```typescript
{{#include ../../examples/ui/overview/quickstart.ts}}
```

```bash
perry run app.ts
```

On macOS, UI builds produce an `.app` bundle so the system recognizes the
application's bundle identity. To compile and
launch separately, use `perry app.ts -o app && open app.app`. The linked
`app` executable is also kept; launch the bundle for desktop use. An explicit
`-o MyApp.app` writes the executable directly inside that bundle.

The bundle includes project assets, localization resources, and the configured
app identity and version. `perry run` launches its executable with the supplied
arguments and terminal input/output. Command-line programs that do not use
`perry/ui` keep their standalone executable output. Optional `--emit-attest`
and `--emit-sandbox` sidecars are written beside the `.app`; the attestation
covers its signed executable in `Contents/MacOS`.

To set the macOS Dock and Finder icon, put `AppIcon.icns` in the project's
`assets/` directory. Perry copies it to the bundle's `Contents/Resources/`
directory and references it in `Info.plist`. The bundle name uses
`[macos].display_name`, then `[project].display_name`, then `[project].name` in
`perry.toml`; without these it uses the output filename. Its version uses
`[project].version`, then `package.json`'s `version`.

## Mental Model

Perry's UI follows the same model as SwiftUI and Flutter: you compose native widgets using stack-based layout containers (`VStack`, `HStack`, `ZStack`), control alignment and distribution, and style widgets via free functions that take the widget handle as their first argument (`textSetColor(label, r, g, b, a)`, `setPadding(stack, ...)`, etc.). If you're coming from web development, the key shift is:

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
| `frameAutosaveName` | string | Remember the desktop window frame under a stable name (optional; empty disables) |
| `frameless` | boolean | Remove title bar (optional) |
| `level` | string | Window z-order: `"floating"`, `"statusBar"`, `"modal"` (optional) |
| `transparent` | boolean | Transparent background (optional) |
| `vibrancy` | string | Native blur material, e.g. `"sidebar"` (optional) |
| `activationPolicy` | string | `"regular"`, `"accessory"` (no dock icon), `"background"` (optional) |

See [Multi-Window](multi-window.md#app-window-properties) for full
documentation on window properties, including the native primitive each
field maps to per platform.

### Remembering the window frame

Set `frameAutosaveName` to a stable name to reopen an app where the user left it:

```typescript
{{#include ../../examples/ui/overview/frame-persistence.ts}}
```

The saved frame takes precedence over `width`, `height`, and `windowState`.
Missing or unreadable saved settings use those launch defaults. Omit the option
or pass an empty string to disable persistence. Names are scoped to the executable
path, so unrelated apps do not share frames; moving the executable starts fresh.
Use a different name for each independent app window. Changing the title does not
change the saved frame.

macOS uses AppKit's native frame autosave for position and size; this option does
not promise restoration of a fullscreen Space. Windows, including WinUI, saves
normal placement and maximized/fullscreen state, and reopens minimized windows
in their previous non-minimized state. GTK4 saves normal size and
maximized/fullscreen state; the compositor controls window position because
[GTK4 has no global positioning API](https://docs.gtk.org/gtk4/migrating-3to4.html).
The option is ignored on mobile, TV, watch, and visionOS backends.

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
