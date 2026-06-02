// #679 Phase 4 — ink source-compat test #4: useRef stable handle.
//
// ink's useRef pattern: store something the component shouldn't
// re-render on (a counter of renders, a DOM-like handle, etc.).
// Validates that .set() doesn't trigger re-render (no STATE_DIRTY
// flip) and that the same handle is returned across renders.

import { useRef, useInput, run, Box, Text, exit } from "perry/tui";

let renderCount = 0;
let storedFinal = 0;

run(() => {
    const ref = useRef(0);
    // Increment the ref every render — would loop forever if writes
    // triggered re-render. ink-shape: useRef writes are silent.
    ref.set(ref.get() + 1);
    storedFinal = ref.get();
    renderCount = renderCount + 1;
    useInput((s: string) => {
        if (s === "q") exit();
    });
    return Box([Text("renders: " + ref.get())]);
});

console.log("RENDER_COUNT_BOUNDED=" + (renderCount < 5));
console.log("FINAL_REF=" + storedFinal);
