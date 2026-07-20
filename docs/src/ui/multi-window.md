# Multi-Window & Window Management

Perry supports creating multiple native windows and controlling their
appearance and behavior. Every snippet below is excerpted from
[`docs/examples/ui/multi_window/snippets.ts`](../../examples/ui/multi_window/snippets.ts) —
CI compiles and runs it on every PR.

## Creating Windows

`Window(title, width, height)` returns a window handle. Call `.setBody()` to
set its content and `.show()` to display it:

```typescript
{{#include ../../examples/ui/multi_window/snippets.ts:create}}
```

## Window Instance Methods

```typescript
{{#include ../../examples/ui/multi_window/snippets.ts:methods}}
```

| Method | Description |
|--------|-------------|
| `setBody(widget)` | Set the root widget of the window |
| `show()` | Show the window |
| `hide()` | Hide without destroying — call `show()` again to reveal |
| `setSize(w, h)` | Resize dynamically |
| `onFocusLost(cb)` | Register a callback that fires when focus leaves the window |
| `close()` | Close and destroy |

## App Window Properties

The main `App({})` config object accepts the same window properties for
building launcher-style, overlay, or utility apps:

```typescript
{{#include ../../examples/ui/multi_window/snippets.ts:app-config}}
```

`App` additionally accepts the optional fields `frameless`, `level`,
`transparent`, `vibrancy`, `activationPolicy`, and `icon`. Each present
field is applied to the window right after creation — before the body
widget is attached, so vibrancy and frameless reconfigure the window ahead
of layout. (`activationPolicy` is also settable at runtime via the
standalone `appSetActivationPolicy(policy)` function.) They map to the
following native primitives:

### `frameless: true`

Removes the window title bar and frame, creating a borderless window.

| Platform | Implementation |
|----------|---------------|
| macOS | `NSWindowStyleMask::Borderless` + movable by background |
| Windows | `WS_POPUP` window style |
| Linux | `set_decorated(false)` |

### `level: "floating" | "statusBar" | "modal" | "normal"`

Controls the window's z-order level relative to other windows.

| Level | Description |
|-------|-------------|
| `"normal"` | Default window level |
| `"floating"` | Stays above normal windows |
| `"statusBar"` | Stays above floating windows |
| `"modal"` | Modal panel level |

| Platform | Implementation |
|----------|---------------|
| macOS | `NSWindow.level` (NSFloatingWindowLevel, etc.) |
| Windows | `SetWindowPos` with `HWND_TOPMOST` |
| Linux | `set_modal(true)` (best-effort) |

### `transparent: true`

Makes the window background transparent, allowing the desktop to show through
non-opaque regions of your UI.

| Platform | Implementation |
|----------|---------------|
| macOS | `isOpaque = false`, `backgroundColor = .clear` |
| Windows | `WS_EX_LAYERED` with `SetLayeredWindowAttributes` |
| Linux | CSS `background-color: transparent` |

### `vibrancy: string`

Applies a native translucent material to the window background. On macOS this
uses the system vibrancy effect; on Windows it uses Mica/Acrylic.

**macOS materials:** `"sidebar"`, `"titlebar"`, `"selection"`, `"menu"`,
`"popover"`, `"headerView"`, `"sheet"`, `"windowBackground"`, `"hudWindow"`,
`"fullScreenUI"`, `"tooltip"`, `"contentBackground"`, `"underWindowBackground"`,
`"underPageBackground"`

| Platform | Implementation |
|----------|---------------|
| macOS | `NSVisualEffectView` with the specified material |
| Windows | `DwmSetWindowAttribute(DWMWA_SYSTEMBACKDROP_TYPE)` — Mica, Acrylic, or Mica Alt depending on material (Windows 11 22H2+) |
| Linux | CSS `alpha(@window_bg_color, 0.85)` (best-effort) |

### `activationPolicy: "regular" | "accessory" | "background"`

Controls whether the app appears in the dock/taskbar.

| Policy | Description |
|--------|-------------|
| `"regular"` | Normal app with dock icon and menu bar (default) |
| `"accessory"` | No dock icon, no menu bar activation — ideal for launchers and utilities |
| `"background"` | Fully hidden from dock and app switcher |

| Platform | Implementation |
|----------|---------------|
| macOS | `NSApp.setActivationPolicy()` |
| Windows | `WS_EX_TOOLWINDOW` (removes from taskbar) |
| Linux | `set_deletable(false)` (best-effort) |

## Platform Notes

| Platform | Implementation |
|----------|---------------|
| macOS | NSWindow |
| Windows | CreateWindowEx (HWND) |
| Linux | GtkWindow |
| Web | Floating `<div>` |
| iOS/Android | Modal view controller / Dialog |

On mobile platforms, "windows" are presented as modal views or dialogs since
mobile apps typically use a single-window model.

## Next Steps

- [Events](events.md) — Keyboard shortcuts
- [Dialogs](dialogs.md) — Modal dialogs and sheets
- [Menus](menus.md) — Menu bar and toolbar
- [UI Overview](overview.md) — Full UI system overview
