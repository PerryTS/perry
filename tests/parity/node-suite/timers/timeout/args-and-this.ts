const events: string[] = [];
setTimeout(function(this: any, a: string, b: string) { events.push(String(this && typeof this) + ":" + a + b); }, 1, "a", "b");
await new Promise(resolve => setTimeout(resolve, 20));
console.log("events:", events.join(","));
