// demonstrates: widgetSetMaxWidth — a capped, centered content column
// docs: docs/src/ui/styling.md
// platforms: macos, linux
// targets: ios-simulator, tvos-simulator, visionos-simulator

// A column is added to the window body FIRST (so it has a parent), then capped
// at 320pt inside a 900pt window. Below the cap it fills; at/above it stops and
// centers. Runnable proof for issue #10167 — the harness exits after one frame.
import {
    App, VStack, Text,
    widgetAddChild, widgetSetMaxWidth, widgetSetHeight,
    widgetSetBackgroundColor,
} from "perry/ui"

const root = VStack(0, [])
widgetSetBackgroundColor(root, 0.1, 0.1, 0.12, 1.0)

const column = VStack(0, [Text("max-width 320, centered")])
widgetAddChild(root, column)
widgetSetBackgroundColor(column, 0.2, 0.5, 0.95, 1.0)
widgetSetHeight(column, 120)
widgetSetMaxWidth(column, 320)

App({
    title: "max-width-centered",
    width: 900,
    height: 300,
    body: root,
})
