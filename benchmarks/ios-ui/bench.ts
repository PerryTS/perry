// iOS UI frame-time benchmark.
//
// Cycles through the workloads a cross-platform UI framework is normally
// judged on, printing a marker before each so the `[frame-stats]` lines the
// runtime emits can be attributed to a workload.
//
// Run with metrics on:
//
//     PERRY_FRAME_STATS=1 perry run benchmarks/ios-ui/bench.ts --target ios
//
// `PERRY_FRAME_STATS_INTERVAL` (default 300 frames) sets the reporting
// cadence. The scroll and text-heavy phases need you to actually drag the
// list — nothing here injects touches, and a stationary list measures the
// idle path, not scrolling.

import {
    App,
    VStack,
    Text,
    ScrollView,
    scrollviewSetChild,
    widgetAddChild,
    widgetClearChildren,
    widgetSetBackgroundColor,
    textSetString,
    onFrame,
} from "perry/ui"

// Rows rebuilt per cycle in the add/remove phase. Large enough that the old
// O(children x every widget ever created) handle scan was visible, small
// enough to stay inside a frame when it isn't.
const CHURN_ROWS = 100
// Labels mutated every frame in the property-update phase.
const LIVE_LABELS = 50
// Rows built once for the text-heavy phase.
const TEXT_ROWS = 300

const PHASE_SECONDS = 8

type Phase = {
    name: string
    // Called once when the phase starts.
    enter: () => void
    // Called every frame while the phase is active.
    tick: (frame: number) => void
}

const content = VStack(4, [])
const scroll = ScrollView()
scrollviewSetChild(scroll, content)

// Labels kept live across frames for the property-update and animation
// phases, so those measure mutation rather than construction.
const liveLabels: unknown[] = []

function clearContent(): void {
    widgetClearChildren(content)
    liveLabels.length = 0
}

function buildLabels(count: number, prefix: string): void {
    for (let i = 0; i < count; i++) {
        const row = Text(`${prefix} ${i}`)
        liveLabels.push(row)
        widgetAddChild(content, row)
    }
}

const phases: Phase[] = [
    {
        // Baseline. Establishes the floor everything else is read against — a
        // p99 that is already bad here is not a workload problem.
        name: "idle",
        enter: (): void => {
            clearContent()
            buildLabels(20, "idle row")
        },
        tick: (): void => {},
    },
    {
        // Frequent property updates: the label-text path, which is the
        // cheapest possible cross-framework UI operation and therefore the
        // clearest read on per-call dispatch overhead.
        name: "property-updates",
        enter: (): void => {
            clearContent()
            buildLabels(LIVE_LABELS, "live")
        },
        tick: (frame: number): void => {
            for (let i = 0; i < liveLabels.length; i++) {
                textSetString(liveLabels[i] as never, `live ${i} @ ${frame}`)
            }
        },
    },
    {
        // Adding/removing many elements. This is the phase the widget-table
        // handle scan used to dominate, and it degraded as the run went on,
        // so a flat p99 across the whole phase is the thing to check.
        name: "add-remove",
        enter: (): void => {
            clearContent()
        },
        tick: (frame: number): void => {
            // Every 6th frame, so the rebuild cost is visible as a spike
            // rather than smeared across every frame.
            if (frame % 6 !== 0) return
            clearContent()
            buildLabels(CHURN_ROWS, "row")
        },
    },
    {
        // Animation: per-frame property churn across many widgets.
        name: "animation",
        enter: (): void => {
            clearContent()
            buildLabels(LIVE_LABELS, "anim")
        },
        tick: (frame: number): void => {
            for (let i = 0; i < liveLabels.length; i++) {
                const t = ((frame + i * 3) % 60) / 60
                widgetSetBackgroundColor(liveLabels[i] as never, t, 0.4, 1 - t, 1)
            }
        },
    },
    {
        // Text-heavy screen. Built once, then static — drag the list during
        // this phase to measure scrolling.
        name: "text-heavy (scroll me)",
        enter: (): void => {
            clearContent()
            for (let i = 0; i < TEXT_ROWS; i++) {
                widgetAddChild(
                    content,
                    Text(
                        `${i}. The quick brown fox jumps over the lazy dog, ` +
                            `and then keeps going so this line has to wrap.`,
                    ),
                )
            }
        },
        tick: (): void => {},
    },
]

let phaseIndex = -1
let phaseStartMs = 0
let frame = 0

function enterPhase(index: number, nowMs: number): void {
    phaseIndex = index
    phaseStartMs = nowMs
    frame = 0
    const phase = phases[index]
    // The runtime's [frame-stats] lines carry no workload label; this marker
    // is what makes them attributable.
    console.log(`=== phase: ${phase.name} ===`)
    phase.enter()
}

function loop(timestampMs: number): void {
    if (phaseIndex < 0) {
        enterPhase(0, timestampMs)
    } else if (timestampMs - phaseStartMs >= PHASE_SECONDS * 1000) {
        enterPhase((phaseIndex + 1) % phases.length, timestampMs)
    }

    phases[phaseIndex].tick(frame)
    frame++

    onFrame(loop)
}

onFrame(loop)

App({
    title: "Perry UI Bench",
    width: 400,
    height: 800,
    body: VStack(0, [scroll]),
})
