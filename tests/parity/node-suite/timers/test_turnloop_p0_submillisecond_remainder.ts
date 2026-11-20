// turnloop P0 loop-statistics probe: a 2 ms timeout whose remaining time is
// below one millisecond when the loop first parks. The pre-P0 park truncated
// that remainder to 0 ms and returned without waiting until the timer was due
// (a spin bounded only by the #1114 throttle); the P0 park waits for the exact
// Instant.
const start = performance.now();
setTimeout(() => console.log("remainder deadline hit"), 2);
while (performance.now() - start < 1.6) {
  // burn the first 1.6 ms synchronously
}
