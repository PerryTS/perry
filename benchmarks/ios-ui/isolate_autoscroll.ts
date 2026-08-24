// Isolation probe: programmatic scrolling + label mutation, no human needed.
//
// #7763 has so far only reproduced with a person dragging the list, which makes
// it expensive to hunt. This drives the scroll offset from the frame callback
// instead, combining the two ingredients the crash needs — scroll-driven layout
// and live label mutation — without touch input.
//
// KNOWN LIMITATION: `setContentOffset` does not run UIScrollView's gesture
// recognizer and does not switch the run loop into UITrackingRunLoopMode, so
// this is not equivalent to a real drag. If it reproduces, we get a
// human-free repro. If it does not, that is itself informative: it narrows the
// trigger to something only a real touch sequence provides (tracking mode,
// deceleration, or the gesture recognizer's own layout work).

import {
    App,
    VStack,
    Text,
    ScrollView,
    scrollviewSetChild,
    scrollviewSetOffset,
    widgetAddChild,
    textSetString,
    onFrame,
} from "perry/ui"

const ROWS = 200
const MUTATE = 50

const content = VStack(4, [])
const scroll = ScrollView()
scrollviewSetChild(scroll, content)

const labels: unknown[] = []
for (let i = 0; i < ROWS; i++) {
    const row = Text(`row ${i}`)
    labels.push(row)
    widgetAddChild(content, row)
}

let frame = 0
let offset = 0
let direction = 1

function loop(): void {
    frame++

    // Sawtooth scroll, fast enough to keep layout continuously busy.
    offset += direction * 40
    if (offset > 3000) {
        direction = -1
    } else if (offset < 0) {
        direction = 1
    }
    scrollviewSetOffset(scroll, 0, offset)

    // Mutate labels while the scroll layout is in flight — the same
    // combination the crashing benchmark phase performs.
    for (let i = 0; i < MUTATE; i++) {
        textSetString(labels[i] as never, `row ${i} f${frame}`)
    }

    if (frame % 600 === 0) {
        console.log(`frames: ${frame} offset: ${offset}`)
    }
    onFrame(loop)
}

onFrame(loop)

App({
    title: "Autoscroll Isolate",
    width: 400,
    height: 800,
    body: VStack(0, [scroll]),
})
