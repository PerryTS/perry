// demonstrates: a styled counter card using the real free-function API
// docs: docs/src/ui/styling.md
// platforms: macos, linux, windows
// targets: ios-simulator, web, wasm

import {
    App,
    Text,
    Button,
    VStack,
    HStack,
    State,
    Spacer,
    textSetFontSize,
    textSetFontFamily,
    textSetColor,
    widgetSetBackgroundColor,
    setPadding,
    setCornerRadius,
} from "perry/ui"

// Note: widgetSetBorderColor / widgetSetBorderWidth are macOS/iOS/Windows
// only — perry-ui-gtk4 doesn't export them (GTK4 borders are CSS-driven).
// Omitted from this demo so it compiles everywhere.

const count = State(0)

const title = Text("Counter")
textSetFontSize(title, 28)
textSetColor(title, 0.1, 0.1, 0.1, 1.0)

const display = Text(`${count.value}`)
textSetFontSize(display, 48)
textSetFontFamily(display, "monospaced")
textSetColor(display, 0.0, 0.478, 1.0, 1.0)

const decBtn = Button("-", () => count.set(count.value - 1))
setCornerRadius(decBtn, 20)
widgetSetBackgroundColor(decBtn, 1.0, 0.231, 0.188, 1.0)

const incBtn = Button("+", () => count.set(count.value + 1))
setCornerRadius(incBtn, 20)
widgetSetBackgroundColor(incBtn, 0.204, 0.78, 0.349, 1.0)

const controls = HStack(8, [decBtn, Spacer(), incBtn])
setPadding(controls, 20)

const container = VStack(16, [title, display, controls])
setPadding(container, 40)
setCornerRadius(container, 16)
widgetSetBackgroundColor(container, 1.0, 1.0, 1.0, 1.0)

App({
    title: "Styled App",
    width: 400,
    height: 300,
    body: container,
})
