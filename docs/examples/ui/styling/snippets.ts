// demonstrates: per-API styling snippets shown in docs/src/ui/styling.md
// docs: docs/src/ui/styling.md
// platforms: macos, linux
// Windows excluded: this complete-file example uses try/catch, which the
// default Windows RS4GC pipeline intentionally refuses until WinEH funclet
// statepoints are supported (#7354).

// Each ANCHOR block below is the exact code that the styling docs page
// renders inline (via {{#include ... :NAME}}). The whole file is compiled
// and run by the doc-tests harness, so every snippet is a tested artifact —
// if anything drifts from the real perry/ui API, CI fails.

// ANCHOR: imports
import {
    App,
    VStack, VStackWithInsets, HStack, Spacer,
    Text, Button,
    textSetColor, textSetFontSize, textSetFontFamily, textSetFontWeight,
    setCornerRadius, setPadding,
    widgetAddChild,
    widgetSetBackgroundColor, widgetSetBackgroundGradient,
    widgetSetBorderColor, widgetSetBorderWidth,
    widgetSetEdgeInsets,
    widgetSetWidth, widgetSetHeight, widgetMatchParentWidth,
    widgetSetOpacity,
    widgetSetControlSize,
    widgetSetTooltip,
    widgetSetEnabled,
} from "perry/ui"
// ANCHOR_END: imports

// ANCHOR: colors
const colored = Text("Colored text")
textSetColor(colored, 1.0, 0.0, 0.0, 1.0)              // r, g, b, a in [0,1]
widgetSetBackgroundColor(colored, 0.94, 0.94, 0.94, 1.0)
// ANCHOR_END: colors

// ANCHOR: fonts
const font = Text("Styled text")
textSetFontSize(font, 24)                  // Font size in points
textSetFontFamily(font, "Menlo")           // Font family name
textSetFontWeight(font, 24, 700)           // Re-set size + weight together
// ANCHOR_END: fonts

// ANCHOR: corner-radius
const rounded = Button("Rounded", () => {})
setCornerRadius(rounded, 12)
// ANCHOR_END: corner-radius

// ANCHOR: borders
const bordered = VStack(0, [])
widgetSetBorderColor(bordered, 0.8, 0.8, 0.8, 1.0)
widgetSetBorderWidth(bordered, 1)
// ANCHOR_END: borders

// ANCHOR: padding
const padded = VStack(8, [Text("Padded content")])
// Both names accept (widget, top, left, bottom, right):
setPadding(padded, 16, 16, 16, 16)
widgetSetEdgeInsets(padded, 10, 20, 10, 20)
// ANCHOR_END: padding

// ANCHOR: sizing
const sized = VStack(0, [])
widgetSetWidth(sized, 300)
widgetSetHeight(sized, 200)
widgetMatchParentWidth(sized) // expand to fill parent's width
// ANCHOR_END: sizing

// ANCHOR: opacity
const dim = Text("Semi-transparent")
widgetSetOpacity(dim, 0.5) // 0.0 to 1.0
// ANCHOR_END: opacity

// ANCHOR: gradient
const grad = VStack(0, [])
// Two RGBA stops + angle (degrees, 0 = top-to-bottom).
widgetSetBackgroundGradient(grad,
    1.0, 0.0, 0.0, 1.0,   // start (red)
    0.0, 0.0, 1.0, 1.0,   // end   (blue)
    0,                    // angle
)
// ANCHOR_END: gradient

// ANCHOR: control-size
const small = Button("Small", () => {})
widgetSetControlSize(small, 0) // 0=mini, 1=small, 2=regular, 3=large
// ANCHOR_END: control-size

// ANCHOR: tooltip
const tip = Button("Hover me", () => {})
widgetSetTooltip(tip, "Click to perform action")
// ANCHOR_END: tooltip

// ANCHOR: enabled
const submit = Button("Submit", () => {})
widgetSetEnabled(submit, 0)  // 0 = disabled, 1 = enabled
// ANCHOR_END: enabled

// ANCHOR: card-helper
function card(children: number[]): number {
  const c = VStackWithInsets(12, 16, 16, 16, 16)
  setCornerRadius(c, 12)
  widgetSetBackgroundColor(c, 1.0, 1.0, 1.0, 1.0)
  widgetSetBorderColor(c, 0.9, 0.9, 0.9, 1.0)
  widgetSetBorderWidth(c, 1)
  for (const child of children) widgetAddChild(c, child)
  return c
}
// ANCHOR_END: card-helper

// Mount everything inside a single window so the program is runnable.
// The doc-tests harness exits after 500 ms in test mode.
App({
    title: "styling-snippets",
    width: 600,
    height: 800,
    body: VStack(12, [
        colored, font, rounded, bordered, padded, sized,
        dim, grad, small, tip, submit,
        card([Text("Title"), Text("Body text")]),
        Spacer(),
    ]),
})
