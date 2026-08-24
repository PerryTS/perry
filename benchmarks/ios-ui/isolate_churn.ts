// Isolation probe: child churn WITHOUT onFrame.
//
// The full bench crashes on device with
// `malloc: pointer being freed was not allocated`. It differs from the
// known-stable minimal app in two ways: it drives `onFrame` (the CADisplayLink
// driver) and it churns children (`widgetClearChildren`, which now releases the
// removed subtree). This probe keeps the churn and drops `onFrame`, so a crash
// here implicates the release path and a clean run implicates the frame driver.

import {
    App,
    VStack,
    Text,
    widgetAddChild,
    widgetClearChildren,
} from "perry/ui"

const ROWS = 100
const content = VStack(4, [])

let round = 0

function churn(): void {
    round++
    widgetClearChildren(content)
    for (let i = 0; i < ROWS; i++) {
        widgetAddChild(content, Text(`row ${i} / round ${round}`))
    }
    if (round % 10 === 0) {
        console.log(`churn rounds: ${round}`)
    }
}

setInterval(churn, 50)

App({
    title: "Churn Isolate",
    width: 400,
    height: 800,
    body: VStack(0, [content]),
})
