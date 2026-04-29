// demonstrates: per-API event snippets shown in docs/src/ui/events.md
// docs: docs/src/ui/events.md
// platforms: macos, linux, windows

import {
    App,
    VStack, HStack, Spacer,
    Text, Button,
    State,
    addKeyboardShortcut, registerGlobalHotkey,
    widgetSetOnClick, widgetSetOnHover, widgetSetOnDoubleClick,
    clipboardRead, clipboardWrite,
    menuCreate, menuAddItem,
} from "perry/ui"

const log = State("(no events yet)")

// ANCHOR: on-click
const greet = Button("Click me", () => {
    log.set("Button clicked")
})

// Or attach a click handler to a non-button widget after creation:
const label = Text("Clickable text")
widgetSetOnClick(label, () => {
    log.set("Text clicked")
})
// ANCHOR_END: on-click

// ANCHOR: on-hover
const hoverBtn = Button("Hover me", () => {})
widgetSetOnHover(hoverBtn, () => {
    log.set("hovered")
})
// ANCHOR_END: on-hover

// ANCHOR: on-double-click
const dbl = Text("Double-click me")
widgetSetOnDoubleClick(dbl, () => {
    log.set("double-clicked!")
})
// ANCHOR_END: on-double-click

// ANCHOR: keyboard
// Cmd+N on macOS, Ctrl+N on other platforms (modifier 1 = Cmd/Ctrl).
addKeyboardShortcut("n", 1, () => {
    log.set("New document")
})

// Cmd+Shift+S — modifiers add: 1 (Cmd/Ctrl) + 2 (Shift) = 3.
addKeyboardShortcut("s", 3, () => {
    log.set("Save as...")
})
// ANCHOR_END: keyboard

// ANCHOR: global-hotkey
// System-wide: fires even when the app is in the background.
// macOS: real Carbon RegisterEventHotKey. Other platforms: no-op.
registerGlobalHotkey("F5", 0, () => {
    log.set("Global F5 hotkey fired")
})

// Cmd+Shift+G (modifiers: 1=Cmd + 2=Shift = 3)
registerGlobalHotkey("g", 3, () => {
    log.set("Global Cmd+Shift+G fired")
})
// ANCHOR_END: global-hotkey

// ANCHOR: menu-shortcut
const fileMenu = menuCreate()
menuAddItem(fileMenu, "New", () => log.set("file/new"))
menuAddItem(fileMenu, "Save As", () => log.set("file/saveAs"))
// ANCHOR_END: menu-shortcut

// ANCHOR: clipboard
// Copy to clipboard
clipboardWrite("Hello, clipboard!")

// Read from clipboard
const text = clipboardRead()
log.set(`clipboard length: ${text.length}`)
// ANCHOR_END: clipboard

App({
    title: "events-snippets",
    width: 480,
    height: 320,
    body: VStack(12, [
        Text(`Last event: ${log.value}`),
        Spacer(),
        HStack(8, [greet, hoverBtn, label]),
        dbl,
    ]),
})
