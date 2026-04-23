// demonstrates: every cross-platform perry/ui widget in one window
// docs: docs/src/ui/widgets.md
// platforms: macos, linux, windows
// targets: ios-simulator, visionos-simulator, tvos-simulator, web, wasm
//
// Rendered deterministically — no timers, no animations, no state changes.
// Captured at fixed size (900x1400) so the screenshot diff is reproducible
// across platforms. Adding a new cross-platform widget? Add a new Section
// here and re-bless the baseline with `doc-tests --bless`.

import {
    App,
    VStack,
    HStack,
    ZStack,
    ScrollView,
    Text,
    Button,
    TextField,
    TextArea,
    SecureField,
    Toggle,
    Slider,
    ProgressView,
    Picker,
    Divider,
    Spacer,
    Section,
    ImageSymbol,
    widgetAddChild,
    scrollviewSetChild,
    pickerAddItem,
    textSetFontSize,
    textSetFontWeight,
    textSetColor,
} from "perry/ui"

function sectionHeader(title: string) {
    const t = Text(title)
    textSetFontSize(t, 16)
    textSetFontWeight(t, 20, 1.0)
    textSetColor(t, 0.2, 0.4, 0.9, 1.0)
    return t
}

// Build the Picker ahead of time so we can populate it.
const picker = Picker((_idx: number) => {})
pickerAddItem(picker, "Apple")
pickerAddItem(picker, "Banana")
pickerAddItem(picker, "Cherry")

// ZStack with two overlapping labels — just enough to prove z-order works.
const zstack = ZStack()
widgetAddChild(zstack, Text("background"))
widgetAddChild(zstack, Text("foreground"))

// Section is a thin grouping container; children attach via widgetAddChild.
const grouping = Section("Grouped controls")
widgetAddChild(grouping, Text("Inside a Section"))

const scroll = ScrollView()
scrollviewSetChild(
    scroll,
    VStack(16, [
        // --- Text & labels -------------------------------------------------
        sectionHeader("Text & labels"),
        Text("Hello, world"),
        Text("Perry UI widget gallery"),
        Divider(),

        // --- Buttons -------------------------------------------------------
        sectionHeader("Buttons"),
        HStack(8, [
            Button("Primary", () => {}),
            Button("Secondary", () => {}),
            Button("Tertiary", () => {}),
        ]),
        Divider(),

        // --- Inputs --------------------------------------------------------
        sectionHeader("Inputs"),
        TextField("type here", (_s: string) => {}),
        TextArea("multi-line\ntext area", (_s: string) => {}),
        SecureField("password", (_s: string) => {}),
        Divider(),

        // --- Selection & progress -----------------------------------------
        sectionHeader("Selection & progress"),
        Toggle("Enabled", (_on: boolean) => {}),
        Slider(0, 100, (_v: number) => {}),
        ProgressView(),
        picker,
        Divider(),

        // --- Layout primitives --------------------------------------------
        sectionHeader("Layout"),
        HStack(8, [Text("left"), Spacer(), Text("right")]),
        zstack,
        Divider(),

        // --- Media ---------------------------------------------------------
        sectionHeader("Media"),
        HStack(12, [
            ImageSymbol("star.fill"),
            ImageSymbol("heart.fill"),
            ImageSymbol("bell.fill"),
        ]),
        Divider(),

        // --- Containers ----------------------------------------------------
        sectionHeader("Containers"),
        grouping,

        Spacer(),
    ]),
)

App({
    title: "Perry UI Gallery",
    width: 900,
    height: 1400,
    body: scroll,
})
