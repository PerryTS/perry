process.on("beforeExit", () => {
  console.log("beforeExit");
  setTimeout(() => console.log("unexpected timeout"), 0).unref();
  setImmediate(() => console.log("unexpected immediate")).unref();
});
process.on("exit", () => console.log("exit"));
