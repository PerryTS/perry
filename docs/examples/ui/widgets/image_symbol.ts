// demonstrates: ImageSymbol for SF Symbol glyphs (macOS/iOS)
// docs: docs/src/ui/widgets.md
// platforms: macos
// targets: ios-simulator, visionos-simulator, tvos-simulator

import { App, HStack, ImageSymbol, widgetSetWidth, widgetSetHeight } from "perry/ui"

const star = ImageSymbol("star.fill")
widgetSetWidth(star, 32)
widgetSetHeight(star, 32)

const heart = ImageSymbol("heart.fill")
const bell = ImageSymbol("bell.fill")

App({
    title: "ImageSymbol",
    width: 400,
    height: 200,
    body: HStack(12, [star, heart, bell]),
})
