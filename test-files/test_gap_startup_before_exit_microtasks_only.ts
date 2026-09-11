// Promise/nextTick work alone must not cause beforeExit to repeat forever.
process.on("beforeExit", () => {
  console.log("beforeExit");
  process.nextTick(() => console.log("tick"));
  Promise.resolve().then(() => console.log("promise"));
});
process.on("exit", () => console.log("exit"));
