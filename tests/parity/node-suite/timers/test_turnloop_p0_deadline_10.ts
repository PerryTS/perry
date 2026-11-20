// turnloop P0 loop-statistics probe: one 10 ms timeout, reached in at most two
// turns with at most one zero-event OS wait.
setTimeout(() => console.log("deadline 10 hit"), 10);
