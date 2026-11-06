// #9403 — `process.on("exit", …)` and `process.exitCode`.
//
// Node:
//   * the code passed to the handlers is the pending `process.exitCode`
//     (5 here), not a hardcoded 0;
//   * the argument is snapshotted at emit time, so a handler that reassigns
//     `process.exitCode` does NOT change the argument a later handler sees…
//   * …but it DOES change the final process status: this program exits 9.
//
// Companion expected-exit file asserts the status is 9.
process.exitCode = 5;

process.on("exit", (code: number) => {
  console.log("h1 code=" + code + " process.exitCode=" + process.exitCode);
  process.exitCode = 9;
});

process.on("exit", (code: number) => {
  console.log("h2 code=" + code + " process.exitCode=" + process.exitCode);
});

console.log("main done");
