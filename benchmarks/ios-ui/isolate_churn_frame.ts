// Isolation probe: child churn driven from `onFrame` instead of `setInterval`.
//
// `isolate_churn.ts` ran 3,230 clear+rebuild rounds from a `setInterval` with no
// crash. The full bench does the same structural mutation from `onFrame` and
// dies with:
//
//   NSInvalidArgumentException: -[__NSArrayM insertObject:atIndex:]:
//   object cannot be nil
//
// The only difference between the two is *where in the run loop* the mutation
// happens: an NSTimer fires at a quiescent point, while a CADisplayLink fires
// inside CoreAnimation's pre-commit phase. This probe changes nothing but the
// driver, so a crash here pins the fault on mutating the view hierarchy from a
// display-link callback rather than on the churn itself.

import {
    App,
    VStack,
    Text,
    ScrollView,
    scrollviewSetChild,
    widgetAddChild,
    widgetClearChildren,
    onFrame,
} from "perry/ui"

const ROWS = 100
const content = VStack(4, [])
// Variable under test: the bench churns a stack that lives inside a
// UIScrollView, this probe originally churned one parented directly to the
// root. Churn-from-onFrame alone did NOT reproduce (640 rounds clean).
const scroll = ScrollView()
scrollviewSetChild(scroll, content)

let frame = 0
let round = 0

function loop(): void {
    // Same cadence as the bench's add-remove phase.
    if (frame % 6 === 0) {
        round++
        widgetClearChildren(content)
        for (let i = 0; i < ROWS; i++) {
            widgetAddChild(content, Text(`row ${i} / round ${round}`))
        }
        if (round % 10 === 0) {
            console.log(`churn rounds: ${round}`)
        }
    }
    frame++
    onFrame(loop)
}

onFrame(loop)

App({
    title: "Churn+Frame Isolate",
    width: 400,
    height: 800,
    body: VStack(0, [scroll]),
})
