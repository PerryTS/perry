// turnloop P0 loop-statistics probe: one 2 ms timeout, reached in at most two
// turns with at most one zero-event OS wait.
setTimeout(() => console.log("deadline 2 hit"), 2);
