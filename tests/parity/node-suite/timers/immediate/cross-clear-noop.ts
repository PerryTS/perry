const events: string[] = [];

const immediate: any = setImmediate(() => events.push("immediate"));
clearTimeout(immediate);
clearInterval(immediate as any);

const timeout: any = setTimeout(() => events.push("timeout"), 1);
clearImmediate(timeout as any);

await new Promise<void>((resolve) => setTimeout(() => resolve(), 20));
console.log("events length:", events.length);
console.log("events sorted:", events.slice().sort().join(","));
