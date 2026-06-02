// #679 Phase 4 — ink source-compat test #6: useMemo caching.
//
// useMemo(fn, deps) caches fn() across renders when deps don't change.
// ink programs use this to avoid recomputing derived data each frame.

import { useMemo, useInput, run, Box, Text, exit } from "perry/tui";

let computeCount = 0;
let lastValue = 0;

run(() => {
    // Deps `[]` — useMemo runs the fn ONCE (first render) and caches
    // the result. computeCount should stay at 1 across multiple frames.
    const v = useMemo(() => {
        computeCount = computeCount + 1;
        return 21 * 2;
    }, []);
    lastValue = v;
    useInput((s: string) => {
        if (s === "q") exit();
    });
    return Box([Text("v: " + v)]);
});

console.log("COMPUTE_COUNT=" + computeCount);
console.log("MEMO_VALUE=" + lastValue);
