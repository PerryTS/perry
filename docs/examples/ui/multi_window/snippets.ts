// demonstrates: per-API multi-window snippets shown in docs/src/ui/multi-window.md
// docs: docs/src/ui/multi-window.md
// platforms: macos, linux, windows

import {
    App,
    VStack,
    Text, Button,
    Window,
} from "perry/ui"

// ANCHOR: create
const settings = Window("Settings", 500, 400)
settings.setBody(VStack(16, [
    Text("Settings panel"),
]))
settings.show()
// ANCHOR_END: create

// ANCHOR: methods
const win = Window("My Window", 600, 400)

win.setBody(Text("Hello"))   // Set the root widget
win.show()                    // Show the window
win.hide()                    // Hide without destroying
win.setSize(800, 600)         // Resize dynamically
win.onFocusLost(() => {       // Callback when the window loses focus
    win.hide()
})
win.close()                   // Close and destroy
// ANCHOR_END: methods

// ANCHOR: app-config
App({
    title: "QuickLaunch",
    width: 600,
    height: 80,
    frameless: true,             // borderless window, movable by background
    level: "floating",           // stays above normal windows
    transparent: true,           // desktop shows through non-opaque regions
    vibrancy: "sidebar",         // native translucent background material
    activationPolicy: "accessory", // no dock icon — launcher-style app
    body: VStack(8, [
        Text("Search..."),
        Button("Open Settings", () => settings.show()),
    ]),
})
// ANCHOR_END: app-config
