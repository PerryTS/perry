// A Timeout handle exposes Symbol.dispose; disposing it clears the timer
// (the explicit-resource-management `using` form relies on this).
const events: string[] = [];
const t = setTimeout(() => events.push("fired"), 5) as any;
console.log("dispose typeof:", typeof t[Symbol.dispose]);
t[Symbol.dispose]();
await new Promise<void>((resolve) => setTimeout(() => resolve(), 25));
console.log("events after dispose:", events.join(",") || "none");
