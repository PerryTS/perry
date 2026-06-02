const calls: string[] = [];

function onceHandler() {
  calls.push("once");
}

function prependedOnceHandler() {
  calls.push("prependedOnce");
}

process.once("evt-once-remove", onceHandler);
console.log("once count before remove:", process.listenerCount("evt-once-remove"));
process.removeListener("evt-once-remove", onceHandler);
console.log("once count after remove:", process.listenerCount("evt-once-remove"));
console.log("once emit after remove:", process.emit("evt-once-remove" as any));

process.once("evt-once-off", onceHandler);
console.log("once off before:", process.listenerCount("evt-once-off"));
process.off("evt-once-off", onceHandler);
console.log("once off after:", process.listenerCount("evt-once-off"));

process.prependOnceListener("evt-prepend-once-remove", prependedOnceHandler);
console.log("prepend once before:", process.listenerCount("evt-prepend-once-remove"));
process.removeListener("evt-prepend-once-remove", prependedOnceHandler);
console.log("prepend once after:", process.listenerCount("evt-prepend-once-remove"));
console.log("calls:", calls.join(","));
