// parity-node-argv: --no-warnings

const refs = {
  before: { name: "before" },
  exit: { name: "exit" },
  removed: { name: "removed" },
};

process.on("beforeExit", () => {
  console.log("event beforeExit");
});

process.finalization.registerBeforeExit(refs.before, (obj, event) => {
  console.log("final beforeExit:", obj === refs.before, event);
});
process.finalization.register(refs.exit, (obj, event) => {
  console.log("final exit:", obj === refs.exit, event);
});
process.finalization.registerBeforeExit(refs.removed, () => {
  console.log("removed beforeExit");
});
process.finalization.register(refs.removed, () => {
  console.log("removed exit");
});

console.log("beforeExit listener count:", process.listenerCount("beforeExit"));
console.log("unregister removed:", process.finalization.unregister(refs.removed) === undefined);
console.log("beforeExit listener count after unregister:", process.listenerCount("beforeExit"));
console.log("body done");
