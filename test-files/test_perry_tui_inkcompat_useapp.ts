// #679 Phase 4 — ink source-compat test #2: useApp imperative exit.
//
// Validates that `useApp()` returns a stable handle whose `.exit()`
// method flips the run-loop's EXIT_FLAG. ink programs often expose a
// `<Button onClick={() => exit()}>Quit</Button>` pattern; the perry/tui
// equivalent (function-call form, pre-JSX-intrinsics) is to grab
// useApp() and call .exit() from a useInput handler.

import { Box, Text, useApp, useInput, run } from "perry/tui";

let exitReason = "none";

run(() => {
    const app = useApp();
    useInput((s: string) => {
        if (s === "q") {
            exitReason = "user-quit";
            app.exit();
        }
    });
    return Box([Text("press q to quit")]);
});

console.log("EXIT_REASON=" + exitReason);

/*
@covers
crates/perry-runtime/src/tui/hooks.rs:
  - js_perry_tui_app_exit
  - js_perry_tui_app_wait_until_exit
  - js_perry_tui_wait_until_exit
crates/perry-runtime/src/tui/run.rs:
  - js_perry_tui_run
*/
