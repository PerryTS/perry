// The first checkpoint is empty. beforeExit then queues work for a later one.
let passes = 0;
console.log("sync");
process.on("beforeExit", () => {
  console.log("beforeExit", passes);
  if (passes++ === 0) {
    process.nextTick(() => console.log("tick"));
    Promise.resolve().then(() => {
      console.log("promise");
      setImmediate(() => console.log("immediate"));
    });
  }
});
process.on("exit", code => console.log("exit", code));
