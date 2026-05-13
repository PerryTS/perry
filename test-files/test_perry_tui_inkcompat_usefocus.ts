// #679 Phase 4 — ink source-compat test #5: useFocus + Tab cycle.
//
// ink's useFocus pattern: multiple form inputs cycle via Tab/Shift-Tab.
// We register three "inputs" (just Text widgets for the smoke test),
// the first auto-focused; the test verifies which one reports
// isFocused=1 across initial render + Tab press.

import { useFocus, useInput, run, Box, Text, exit } from "perry/tui";

let finalFocused: number = -1;

run(() => {
    const a = useFocus(1, 1); // autoFocus, isActive
    const b = useFocus(0, 1);
    const c = useFocus(0, 1);
    // Encode which one is focused as 0/1/2 for the post-run assertion.
    if (a > 0) finalFocused = 0;
    else if (b > 0) finalFocused = 1;
    else if (c > 0) finalFocused = 2;
    else finalFocused = -1;
    useInput((s: string) => {
        // 'q' to quit; Tab handled implicitly by drain_input.
        if (s === "q") exit();
    });
    return Box([
        Text(a > 0 ? "[A]" : " A "),
        Text(b > 0 ? "[B]" : " B "),
        Text(c > 0 ? "[C]" : " C "),
    ]);
});

console.log("FINAL_FOCUSED=" + finalFocused);

/*
@covers
crates/perry-runtime/src/tui/hooks.rs:
  - js_perry_tui_focus_next
  - js_perry_tui_focus_previous
crates/perry-runtime/src/tui/input.rs:
  - js_perry_tui_exit
*/
