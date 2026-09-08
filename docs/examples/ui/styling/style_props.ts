// demonstrates: StyleProps type for issue #185 Phase C — typed style
// objects authored once + applied via individual setters today,
// future-compatible with the inline `style: {...}` codegen pass.
// docs: docs/src/ui/styling.md
// platforms: macos, linux, windows
// targets: ios-simulator, tvos-simulator, watchos-simulator, web, wasm, android

import {
    App, VStack, Button,
    StyleProps,
    widgetSetBackgroundColor, widgetSetBorderColor, widgetSetBorderWidth,
    setCornerRadius, setPadding, widgetSetOpacity,
    widgetSetShadow,
} from "perry/ui"

// Author the style as a typed object — IDE autocompletes every prop,
// the type catches typos at compile time, and the structure exactly
// matches the future inline syntax. When the Phase C codegen pass
// lands, this same shape becomes:
//
//     const card = Button("Save", () => {}, { style: cardStyle })
//
// without renaming any prop. Until then, apply the style via the
// individual setters below.
const cardStyle: StyleProps = {
    backgroundColor: { r: 0.231, g: 0.510, b: 0.965, a: 1.0 },
    borderColor: { r: 0.0, g: 0.0, b: 0.0, a: 0.1 },
    borderWidth: 1,
    borderRadius: 8,
    padding: 12,
    opacity: 0.95,
    shadow: {
        color: { r: 0.0, g: 0.0, b: 0.0, a: 0.25 },
        blur: 12,
        offsetX: 0,
        offsetY: 4,
    },
}

const card = Button("Save", () => {
    console.log("saved")
})

// Manually unpack each prop into its setter call. This is the verbose
// pattern; Phase C will replace it with `Button("Save", () => {}, { style: cardStyle })`.
const bg = cardStyle.backgroundColor
if (bg && typeof bg !== "string") {
    widgetSetBackgroundColor(card, bg.r, bg.g, bg.b, bg.a ?? 1.0)
}
const bc = cardStyle.borderColor
if (bc && typeof bc !== "string") {
    widgetSetBorderColor(card, bc.r, bc.g, bc.b, bc.a ?? 1.0)
}
if (cardStyle.borderWidth !== undefined) {
    widgetSetBorderWidth(card, cardStyle.borderWidth)
}
if (cardStyle.borderRadius !== undefined) {
    setCornerRadius(card, cardStyle.borderRadius)
}
if (typeof cardStyle.padding === "number") {
    setPadding(card, cardStyle.padding)
}
if (cardStyle.opacity !== undefined) {
    widgetSetOpacity(card, cardStyle.opacity)
}
const sh = cardStyle.shadow
if (sh && sh.color && typeof sh.color !== "string") {
    widgetSetShadow(
        card,
        sh.color.r, sh.color.g, sh.color.b, sh.color.a ?? 1.0,
        sh.blur ?? 0,
        sh.offsetX ?? 0,
        sh.offsetY ?? 0,
    )
}

App({
    title: "StyleProps Demo",
    width: 320,
    height: 240,
    body: VStack(16, [card]),
})
