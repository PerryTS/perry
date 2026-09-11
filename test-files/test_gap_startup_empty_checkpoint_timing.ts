// No imports/exports: Perry's first entry checkpoint has no ESM promise setup.
process.on("beforeExit", () => {
  console.log("loop started", performance.nodeTiming.loopStart >= 0);
});
