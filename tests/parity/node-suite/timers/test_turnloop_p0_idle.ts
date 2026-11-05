// turnloop P0 loop-statistics probe: idle for 200 ms on one timeout. A quiet
// deadline wait is one OS wait, not a millisecond tick.
setTimeout(() => console.log("idle deadline hit"), 200);
