// #679 Phase 4 — ink source-compat test #7: useEffect with deps array.
//
// useEffect(fn, []) runs fn once on first render; useEffect(fn) (no
// deps array) runs every render. ink programs use the former for
// "mount" side effects (subscribing to an event, opening a connection)
// and the latter rarely.

import { useEffect, useState, useInput, run, Box, Text, exit } from "perry/tui";

let effectCount = 0;

run(() => {
    const [n, _setN] = useState(0);
    useEffect(() => {
        effectCount = effectCount + 1;
    }, []);
    useInput((s: string) => {
        if (s === "q") exit();
    });
    return Box([Text("n=" + n + " effects=" + effectCount)]);
});

// effectCount should be 1 — useEffect with [] runs only on the
// first render across the entire run loop.
console.log("EFFECT_COUNT=" + effectCount);
