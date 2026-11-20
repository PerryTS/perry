// Issue #610 — smoke test that exercises the foreach_register codegen
// path and the runtime state↔foreach binding propagation. The actual
// re-render needs the macOS UI runtime, but we can verify here that:
//   1. The state desugar rewrites ForEach(stateBinding, render).
//   2. The runtime registers the binding (no link-time undefined refs).
//   3. state.set() runs without crashing the binding-walk path.
import { App, VStack, Text, Button, ForEach, state } from "perry/ui";

const count = state(2);
console.log("initial count.get():", count.get());
// Force a state update BEFORE App() to verify the dispatch chain runs
// without crashing. (Under PERRY_UI_TEST_MODE the App is created but
// the timer exits before any user interaction; the binding-walk path
// fires here because the foreach binding hasn't been registered yet —
// state.set just updates STATE_VALUES with no listeners to dispatch
// to. We just want to verify the basic path doesn't crash.)
count.set(5);
console.log("after set, count.get():", count.get());
App({
    title: "ForEach Smoke",
    width: 400,
    height: 400,
    body: VStack(8, [
        count.text(),
        ForEach(count, (i: number) => Text("row " + i)),
        Button("set 5", () => { count.set(5); }),
    ]),
});
