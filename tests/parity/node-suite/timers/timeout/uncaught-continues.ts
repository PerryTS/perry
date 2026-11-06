const events: string[] = [];
process.once("uncaughtException", (err: any) => events.push("caught:" + err.message));
setTimeout(() => { throw new Error("boom"); }, 1);
setTimeout(() => events.push("after"), 2);
await new Promise<void>((resolve) => setTimeout(() => resolve(), 20));
console.log("events:", events.join("|"));
