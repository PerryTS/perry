// Phase 2 v3.3 macOS smoke: showToast + setText link end-to-end on
// `--target macos` (and exit cleanly under PERRY_UI_TEST_MODE=1).
//
// Pre-fix: perry_arkts_show_toast / perry_arkts_set_text only existed
// when `feature = "ohos-napi"` was on, so this file failed to link on
// macOS with `Undefined symbols`. Post-fix: cross-platform stubs in
// `perry-runtime/src/ui_text_registry.rs` always provide the symbols
// and route to the macOS-side handler registered in app_run.

import { App, VStack, Text, Button, setText, showToast } from "perry/ui";

let count = 0;

App({
    title: "Toast Demo",
    width: 400,
    height: 300,
    body: VStack([
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
