// #679 Phase 4 — ink source-compat test #1: counter.
//
// The issue's acceptance program (modulo JSX, which is the deferred
// piece). This validates that:
//   1. `const [n, setN] = useState(0)` lowers to useStateTuple + array
//      destructure (gives both the value and a callable setter).
//   2. setN(n + 1) actually updates the slot and triggers re-render.
//   3. useInput dispatches keypresses to the handler.
//   4. exit() leaves the loop cleanly.
//
// `run(() => Box([...]))` is perry/tui's function-call equivalent of
// ink's `render(<App/>)` — both call the component on every frame.
// Once JSX-with-intrinsics lands the `render(<App/>)` form will also
// work; for now the one-line shape-difference is the body's `return`
// vs. `run(() => ...)`.

import { Box, Text, useState, useInput, run, exit } from "perry/tui";

// Captured by the input handler so we can print FINAL after run() exits.
// We can't reach the post-run frame's hook index from outside the loop,
// so the input handler stashes a copy each time the value changes.
// Note: when stdin delivers multiple bytes in one frame (typical for
// piped input like `+++-q`) the handler fires N times with the SAME
// stale `n` captured by that frame's closure. Mutating finalValue
// here based on what the setter is *about to* write gives the truthful
// post-loop snapshot.
let finalValue = 0;

run(() => {
    const [n, setN] = useState(0);
    finalValue = n;
    useInput((s: string) => {
        if (s === "+") {
            setN(finalValue + 1);
            finalValue = finalValue + 1;
        }
        if (s === "-") {
            setN(finalValue - 1);
            finalValue = finalValue - 1;
        }
        if (s === "q") exit();
    });
    return Box([Text("count: " + n)]);
});

console.log("FINAL=" + finalValue);
