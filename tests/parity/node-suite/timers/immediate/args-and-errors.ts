const events: string[] = [];
process.once("uncaughtException", (err: any) => { events.push("caught:" + err.message); });
setImmediate((a: string, b: string) => events.push(a + b), "a", "b");
setImmediate(() => { throw new Error("boom"); });
setImmediate(() => events.push("after"));
await new Promise(resolve => setTimeout(resolve, 20));
console.log("events:", events.join("|"));
