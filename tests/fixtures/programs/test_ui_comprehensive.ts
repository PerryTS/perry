import {
  App, VStack, HStack, Text, Button, State, Spacer, Divider,
  TextField, Toggle, Slider, ForEach, ScrollView,
  VStackWithInsets, HStackWithInsets,
  textSetString, textSetColor, textSetFontSize, textSetFontWeight, textSetSelectable,
  buttonSetBordered, buttonSetTitle,
  textfieldFocus, textfieldSetString,
  scrollviewSetChild, scrollviewScrollTo, scrollviewGetOffset, scrollviewSetOffset,
  clipboardRead, clipboardWrite,
  addKeyboardShortcut,
  menuCreate, menuAddItem, widgetSetContextMenu,
  openFileDialog,
  appSetMinSize, appSetMaxSize,
  widgetAddChild, widgetAddChildAt, widgetClearChildren, widgetSetHidden,
} from "perry/ui"

// --- Reactive states ---
const count = State(0)
const a = State(3)
const b = State(5)
const dark = State(0)
const listSize = State(3)
const sliderVal = State(50)

// --- Section 1: Styled header ---
const header = Text("Perry UI Comprehensive Test")
textSetFontSize(header, 20)
textSetFontWeight(header, 20, 1.0)
textSetColor(header, 0.2, 0.4, 0.9, 1.0)
textSetSelectable(header, 1)

// --- Section 8: Mutable text target ---
const mutableText = Text("Original text")
const mutableBtn = Button("Original button", () => {})
buttonSetBordered(mutableBtn, 0)

// --- Section 10: ScrollView with tall content ---
const scroll = ScrollView()
const scrollContent = VStack(4, [
  Text("Scroll item 1"),
  Text("Scroll item 2"),
  Text("Scroll item 3"),
  Text("Scroll item 4"),
  Text("Scroll item 5"),
  Text("Scroll item 6"),
  Text("Scroll item 7"),
  Text("Scroll item 8"),
])
scrollviewSetChild(scroll, scrollContent)

// --- Section 9: Widget tree manipulation ---
const dynamicContainer = VStack(4, [
  Text("Dynamic child 1"),
])
const hiddenText = Text("I am hidden")
widgetSetHidden(hiddenText, 1)

// --- Section 13: Context menu ---
const menuTarget = Text("Right-click me")
const menu = menuCreate()
menuAddItem(menu, "Copy", () => { console.log("Menu: Copy") })
menuAddItem(menu, "Paste", () => { console.log("Menu: Paste") })
widgetSetContextMenu(menuTarget, menu)

// --- Section 15: Custom insets ---
const insetStack = VStackWithInsets(8, 10, 20, 10, 20)
widgetAddChild(insetStack, Text("Inset content"))

const insetHStack = HStackWithInsets(8, 5, 10, 5, 10)
widgetAddChild(insetHStack, Text("Left"))
widgetAddChild(insetHStack, Text("Right"))

// --- Build the app ---
const app = App({
  title: "Perry UI Comprehensive Test",
  width: 700,
  height: 800,
  body: VStack(12, [
    // Section 1: Header
    header,
    Divider(),

    // Section 2: Counter
    Text(`Count: ${count.value}`),
    HStack(8, [
      Button("+", () => count.set(count.value + 1)),
      Button("-", () => count.set(count.value - 1)),
    ]),
    Divider(),

    // Section 3: Multi-state text
    Text(`${a.value} + ${b.value} = ${a.value + b.value}`),
    HStack(8, [
      Button("a+1", () => a.set(a.value + 1)),
      Button("b+1", () => b.set(b.value + 1)),
    ]),
    Divider(),

    // Section 4: Two-way slider binding
    Slider(0, 100, sliderVal.value, (v: number) => sliderVal.set(v)),
    Button("Set slider to 75", () => sliderVal.set(75)),
    Divider(),

    // Section 5: Conditional rendering
    dark.value ? Text("Dark Mode ON") : Text("Dark Mode OFF"),
    Toggle("Dark mode", (on: boolean) => dark.set(on ? 1 : 0)),
    Divider(),

    // Section 6: ForEach dynamic list
    ForEach(listSize, (i: number) => Text(`List item ${i}`)),
    HStack(8, [
      Button("Add item", () => listSize.set(listSize.value + 1)),
      Button("Remove item", () => listSize.set(listSize.value - 1)),
    ]),
    Divider(),

    // Section 7: Input controls
    TextField("Type something...", (text: string) => {
      console.log("TextField:", text)
    }),
    Divider(),

    // Section 8: Dynamic mutation
    mutableText,
    mutableBtn,
    HStack(8, [
      Button("Change text", () => {
        textSetString(mutableText, "Updated text!")
        textSetColor(mutableText, 0.9, 0.1, 0.1, 1.0)
      }),
      Button("Change button", () => {
        buttonSetTitle(mutableBtn, "New title!")
        buttonSetBordered(mutableBtn, 1)
      }),
    ]),
    Divider(),

    // Section 9: Widget tree manipulation
    dynamicContainer,
    hiddenText,
    HStack(8, [
      Button("Add child", () => {
        widgetAddChild(dynamicContainer, Text("Added child"))
      }),
      Button("Clear children", () => {
        widgetClearChildren(dynamicContainer)
      }),
      Button("Show hidden", () => {
        widgetSetHidden(hiddenText, 0)
      }),
    ]),
    Divider(),

    // Section 10: ScrollView
    scroll,
    Button("Scroll to top", () => {
      scrollviewSetOffset(scroll, 0)
    }),
    Divider(),

    // Section 11: Clipboard
    HStack(8, [
      Button("Copy to clipboard", () => {
        clipboardWrite("Hello from Perry!")
        console.log("Copied to clipboard")
      }),
      Button("Read clipboard", () => {
        const text = clipboardRead()
        console.log("Clipboard:", text)
      }),
    ]),
    Divider(),

    // Section 13: Context menu target
    menuTarget,
    Divider(),

    // Section 14: Window constraints (applied via button since app blocks)
    Button("Set window constraints", () => {
      appSetMinSize(app, 400, 300)
      appSetMaxSize(app, 1200, 900)
      console.log("Window constraints applied")
    }),
    Divider(),

    // Section 15: Custom insets
    insetStack,
    insetHStack,

    Spacer(),
  ])
})

// Section 12: Keyboard shortcut (Cmd+N)
addKeyboardShortcut("n", 1, () => {
  console.log("Cmd+N pressed!")
})

// Section: File dialog
addKeyboardShortcut("o", 1, () => {
  openFileDialog((path: string) => {
    console.log("Selected file:", path)
  })
})
