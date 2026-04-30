// demonstrates: cross-platform showToast + setText (Phase 2 v3.3, v0.5.408)
// docs: docs/src/ui/state.md
// platforms: macos
// targets:

import { App, VStack, Text, Button, setText, showToast } from "perry/ui";

let count = 0;

App({
    title: "Toast + setText demo",
    width: 400,
    height: 300,
    body: VStack(16, [
        Text("Count: 0", "counter"),
        Button("Increment", () => {
            count++;
            setText("counter", "Count: " + count);
            showToast("Incremented to " + count);
        }),
        Button("Reset", () => {
            count = 0;
            setText("counter", "Count: 0");
            showToast("Reset!");
        }),
    ]),
});
