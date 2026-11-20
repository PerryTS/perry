// Issue #610 — `ForEach(state<number>, render)` should re-render when
// the bound state changes via .set(). Regression test for the runtime
// foreach_register / state_set integration. Doesn't actually exercise
// the UI render path (that needs the macOS UI runtime); just verifies
// the codegen lowers the rewrite without compile errors and the
// runtime registry FFI symbols resolve at link.
import { App, VStack, Text, Button, ForEach, state } from "perry/ui";

const count = state(2);
App({
    title: "ForEach Probe",
    width: 400,
    height: 400,
    body: VStack(8, [
        count.text(),
        ForEach(count, (i: number) => Text("row " + i)),
        Button("set count to 5", () => {
            count.set(5);
            console.log("count is now:", count.get());
        }),
    ]),
});
