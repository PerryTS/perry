const events: string[] = [];
const timeout: any = setTimeout(() => events.push("timeout"), 5);
console.log("timeout dispose type:", typeof timeout[Symbol.dispose]);
timeout[Symbol.dispose]?.();
const immediate: any = setImmediate(() => events.push("immediate"));
console.log("immediate dispose type:", typeof immediate[Symbol.dispose]);
immediate[Symbol.dispose]?.();
await new Promise(resolve => setTimeout(resolve, 20));
console.log("events:", events.join(",") || "none");
