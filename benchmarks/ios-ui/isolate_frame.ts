// Isolation probe: onFrame WITHOUT child churn.
//
// Companion to `isolate_churn.ts`. That probe ran 3,230 clear+rebuild rounds
// (~323k widgets) with no crash, so the release path is not the fault. This one
// keeps the `onFrame` loop and the frame-metrics reporting and does no
// structural mutation at all, isolating the CADisplayLink driver.

import { App, VStack, Text, textSetString, onFrame } from "perry/ui"

const label = Text("frame 0")
const content = VStack(4, [label])

let frames = 0

function loop(timestampMs: number, deltaMs: number): void {
    frames++
    // A cheap property write, so the callback does something observable
    // without touching the view hierarchy's structure.
    textSetString(label, `frame ${frames} dt=${deltaMs.toFixed(2)}`)
    if (frames % 600 === 0) {
        console.log(`frames: ${frames} t=${timestampMs.toFixed(0)}`)
    }
    onFrame(loop)
}

onFrame(loop)

App({
    title: "Frame Isolate",
    width: 400,
    height: 800,
    body: VStack(0, [content]),
})
