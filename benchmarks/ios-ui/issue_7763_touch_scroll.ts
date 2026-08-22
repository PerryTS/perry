// Reproduction for #7763: mutate a UIScrollView-hosted UIStackView while
// real touch-driven scrolling is active. Programmatic setContentOffset does
// not exercise the UIKit gesture/tracking path that triggers the crash.
//
//     PERRY_NO_AUTO_OPTIMIZE=1 perry run ios \
//       benchmarks/ios-ui/issue_7763_touch_scroll.ts --device <UDID>

import {
    App,
    VStack,
    Text,
    ScrollView,
    scrollviewSetChild,
    widgetAddChild,
    widgetClearChildren,
    textSetString,
    onFrame,
} from "perry/ui"

const ROWS = 100
const LIVE_LABELS = 50
// Bound the structural churn so this isolates the UIKit crash instead of the
// separate retained-widget issue on branches that predate widget tombstones.
const CHURN_UNTIL_FRAME = 600
const content = VStack(4, [])
const scroll = ScrollView()
scrollviewSetChild(scroll, content)

const labels: unknown[] = []
let frame = 0

function rebuild(): void {
    widgetClearChildren(content)
    labels.length = 0
    for (let i = 0; i < ROWS; i++) {
        const row = Text(`row ${i} @ ${frame}`)
        labels.push(row)
        widgetAddChild(content, row)
    }
}

rebuild()

function loop(): void {
    frame++
    for (let i = 0; i < LIVE_LABELS; i++) {
        textSetString(labels[i] as never, `row ${i} @ ${frame}`)
    }
    if (frame <= CHURN_UNTIL_FRAME && frame % 6 === 0) {
        rebuild()
    }
    if (frame % 600 === 0) {
        console.log(`issue-7763 frames: ${frame}`)
    }
    onFrame(loop)
}

onFrame(loop)

App({
    title: "Issue 7763 Touch Scroll",
    width: 400,
    height: 800,
    body: VStack(0, [scroll]),
})
