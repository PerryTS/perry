// demonstrates: per-API tray-icon snippets shown in
// docs/src/ui/tray.md
// docs: docs/src/ui/tray.md
// platforms: macos, linux, windows
// run: false
//
// `run: false` because headless macOS CI runners hit a
// CGSConnectionByID assertion (window-server not available without
// a logged-in session). Compile-link is enough to certify the FFI
// surface, which is the contract this file guards.

import {
    App,
    VStack,
    Text,
    State,
    menuCreate,
    menuAddItem,
    menuAddSeparator,
    trayCreate,
    traySetIcon,
    traySetTooltip,
    trayAttachMenu,
    trayOnClick,
    trayDestroy,
} from "perry/ui"

const status = State("ready")

// ANCHOR: tray-basic
// Build the tray BEFORE App() — the tray icon installs while the
// runtime is starting up, so it's already live when the main window
// appears.
const tray = trayCreate("")  // empty path → "●" placeholder
traySetTooltip(tray, "My App")

// Right-click (or left-click on macOS) opens the menu attached below.
const menu = menuCreate()
menuAddItem(menu, "Show", () => status.set("tray/show"))
menuAddSeparator(menu)
menuAddItem(menu, "Quit", () => status.set("tray/quit"))
trayAttachMenu(tray, menu)

// Optional: left-click handler. On macOS the menu pops on left-click,
// so this fires only when no menu is attached. On Windows / Linux,
// left-click and the menu are independent — typical usage is
// "left-click → show main window, right-click → menu".
trayOnClick(tray, () => {
    status.set("tray/click")
})
// ANCHOR_END: tray-basic

// ANCHOR: tray-icon-update
// Hot-swap the icon. The path can be a PNG (every platform), .icns
// (macOS), or .ico (Windows). Empty path is a no-op.
traySetIcon(tray, "./assets/tray.png")
// ANCHOR_END: tray-icon-update

// ANCHOR: tray-destroy
// Remove the tray icon. After this, the handle is dead — set_icon /
// set_tooltip / attach_menu calls become no-ops.
trayDestroy(tray)
// ANCHOR_END: tray-destroy

App({
    title: "Tray Demo",
    width: 320,
    height: 200,
    body: VStack([Text("status", "status")]),
})
