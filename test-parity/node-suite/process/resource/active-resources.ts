// process.getActiveResourcesInfo() returns a string[] of names of libuv
// handles keeping the loop alive. Timers are reported as "Timeout".
console.log("is array:", Array.isArray(process.getActiveResourcesInfo()));
const before = process.getActiveResourcesInfo().filter((name) => name === "Timeout").length;
const timeout = setTimeout(() => {}, 1000);
const interval = setInterval(() => {}, 1000);
const during = process.getActiveResourcesInfo();
clearTimeout(timeout);
clearInterval(interval);
const after = process.getActiveResourcesInfo().filter((name) => name === "Timeout").length;
console.log("includes timeout:", during.includes("Timeout"));
console.log("clears timeout:", after <= before);
