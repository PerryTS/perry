# Tray Icon

Perry ships a cross-platform system tray API on `perry/ui` (issue #490).
The same six functions work on every desktop target — macOS, Windows,
Linux/GTK4 — and link as no-ops on the mobile / embedded backends.

The API is **handle-based** and free-function: build a tray with
`trayCreate(iconPath)`, attach a context menu built with the existing
`menuCreate` / `menuAddItem` API via `trayAttachMenu(tray, menu)`, and
register a left-click callback with `trayOnClick`.

## Basic Usage

```typescript
{{#include ../../examples/ui/tray/snippets.ts:tray-basic}}
```

## API

| Function | Description |
|----------|-------------|
| `trayCreate(iconPath: string): Widget` | Create the tray icon. `iconPath` is a filesystem path to a PNG (or `.icns` on macOS, `.ico` on Windows). Pass `""` to use a "●" placeholder. |
| `traySetIcon(tray, iconPath)` | Hot-swap the icon image. Empty path is a no-op. |
| `traySetTooltip(tray, tooltip)` | Set the tooltip text shown on hover. |
| `trayAttachMenu(tray, menu)` | Attach a context menu (built with `menuCreate` / `menuAddItem`). Right-click — or left-click on macOS — opens the menu. |
| `trayOnClick(tray, callback)` | Register a left-click handler. On macOS the menu pops on left-click, so this only fires when no menu is attached; on Windows / Linux, left-click and menu are independent. |
| `trayDestroy(tray)` | Remove the icon. The handle stays valid (subsequent setters are no-ops) so existing closures don't crash. |

## Updating the Icon

```typescript
{{#include ../../examples/ui/tray/snippets.ts:tray-icon-update}}
```

## Removal

```typescript
{{#include ../../examples/ui/tray/snippets.ts:tray-destroy}}
```

## Platform Notes

| Platform | Backend | Notes |
|----------|---------|-------|
| **macOS** | `NSStatusItem` from `NSStatusBar.system` | Icon appears top-right of the menu bar. Click auto-pops the attached menu. Tooltip routes through the button's `toolTip`. PNG and `.icns` paths supported. Icons are rendered as templates — single-color glyphs adapt to light/dark mode. |
| **Windows** | `Shell_NotifyIconW` + `TrackPopupMenu` | Icon appears in the notification area (bottom-right). Left-click → `onClick` callback. Right-click → menu. PNG and `.ico` paths supported (PNG via `LoadImageW` with `LR_LOADFROMFILE`). `trayCreate` must come after `App({...})` since the tray reuses the main window's `WndProc`. |
| **Linux/GTK4** | StatusNotifierItem (KSNI) over D-Bus | Works on KDE Plasma, GNOME-with-`appindicator`-extension, XFCE, Cinnamon, MATE, Budgie, LXQt out of the box. Vanilla GNOME without the extension keeps the service alive but the icon doesn't render — a one-line warning logs at create time. |
| **iOS / tvOS / visionOS / watchOS** | no-op | These platforms have no system-tray concept. Calls link cleanly and return `0` / no-op so cross-platform code compiles unchanged. |
| **Android** | no-op | Android's "tray" is the notifications shade, which is a different concept. The functions link as no-ops. |
| **HarmonyOS** | no-op | Auto-stubbed at compile time. |
| **Web** | no-op (warns) | Browser tabs have no tray equivalent. |

## Click vs. Menu

Different desktops have different click conventions; Perry exposes both
hooks so a single TypeScript app can do the right thing everywhere:

| Platform | Left-click | Right-click |
|----------|-----------|-------------|
| **macOS** | Pops the attached menu | Same as left-click |
| **Windows** | Fires `onClick` | Pops the attached menu |
| **Linux** | Fires `onClick` (KSNI `activate`) | Pops the attached menu |

The typical pattern: use `onClick` to "show / focus the main window" and
`attachMenu` for the user-facing actions. macOS users will see the menu
pop on every click, which is the platform-native behavior.

## Common Patterns

### Background app (no Dock icon, tray-only)

On macOS, set the activation policy to `"accessory"` so the app has no
Dock icon and lives only as a tray-resident process. (See the
[platform docs](../platforms/macos.md) for activation-policy details.)

### Building the menu after the tray

The menu lookup on every backend happens at click time, not at attach
time. This means you can rebuild the menu (`menuClear` + fresh
`menuAddItem` calls) between user clicks — the new menu wins on the
next click without re-attaching.

## Next Steps

- [Menus](menus.md) — Full menu / submenu / shortcut API used by `trayAttachMenu`
- [State Management](state.md) — Make tray menu items react to app state
- [Multi-Window](multi-window.md) — Show / hide windows from tray actions
