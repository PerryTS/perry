const events: string[] = [];
const im = setImmediate(() => events.push("immediate"));
clearImmediate(im);
clearImmediate(null as any);
setImmediate(() => events.push("after"));
await new Promise(resolve => setTimeout(resolve, 20));
console.log("events:", events.join(","));
