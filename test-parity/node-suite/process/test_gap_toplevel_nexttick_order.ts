// The entry is an ES module because its package.json says "type": "module",
// though it has no import/export: Node evaluates it inside a microtask
// checkpoint, so promise and queueMicrotask jobs queued at top level run
// before the nextTick queue (a CommonJS entry runs the ticks first).
const out: string[] = [];
Promise.resolve().then(() => { out.push("promise1"); process.nextTick(() => out.push("tick-in-promise")); }).then(() => out.push("promise2"));
process.nextTick(() => { out.push("tick1"); Promise.resolve().then(() => out.push("promise-in-tick")); });
queueMicrotask(() => out.push("micro"));
process.nextTick(() => out.push("tick2"));
out.push("sync");
setTimeout(() => console.log(out.join("\n")), 5);
