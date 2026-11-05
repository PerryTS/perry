const events: string[] = [];
setImmediate(() => events.push("immediate"));
setTimeout(() => events.push("timeout"), 0);
await new Promise(resolve => setTimeout(resolve, 30));
console.log("events length:", events.length);
console.log("events sorted:", events.slice().sort().join(","));
