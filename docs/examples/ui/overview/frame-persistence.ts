// demonstrates: opt-in desktop window frame persistence across launches
// docs: docs/src/ui/overview.md
// platforms: macos, linux, windows

import { App, Text } from "perry/ui"

App({
    title: "Remember my window",
    width: 800,
    height: 600,
    windowState: "normal",
    frameAutosaveName: "main",
    body: Text("Move or resize this window, close it, and launch again."),
})
