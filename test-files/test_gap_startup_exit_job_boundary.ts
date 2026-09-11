process.on("exit", () => {
  console.log("exit");
  process.nextTick(() => console.log("unexpected tick"));
  setImmediate(() => console.log("unexpected immediate"));
  Promise.resolve().then(() => console.log("exit promise"));
});
