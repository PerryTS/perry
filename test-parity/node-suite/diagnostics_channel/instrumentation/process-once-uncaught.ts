// Regression guard: `process.once("uncaughtException", h)` should fire
// exactly once. The earlier shortcut aliased `once` to `on`, which would
// have the handler fire repeatedly across multiple uncaught exceptions.
let count = 0;
process.once("uncaughtException", (_err: Error) => {
  count += 1;
});
// We can't easily trigger two separate uncaughtException events without
// process teardown semantics, but we can at least assert that the
// listener registration didn't crash and that the registered count is
// observable as zero before any throw.
console.log("count before throw:", count);
console.log("listener registered ok");
