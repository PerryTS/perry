const { getEnvironmentData, parentPort } = require("node:worker_threads");

const value = getEnvironmentData("builtins");
if (!(value instanceof Map)) {
  parentPort.postMessage({
    map: false,
    date: false,
    set: false,
    dateValue: "missing",
    setValue: "missing",
    localMutation: false,
  });
} else {
  const date = value.get("date");
  const set = value.get("set");
  value.set("worker-only", true);
  parentPort.postMessage({
    map: true,
    date: date instanceof Date,
    set: set instanceof Set,
    dateValue: date instanceof Date ? date.toISOString() : "missing",
    setValue: set instanceof Set ? Array.from(set).join(",") : "missing",
    localMutation: value.has("worker-only"),
  });
}
