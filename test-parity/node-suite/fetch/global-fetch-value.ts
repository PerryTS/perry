const globalFetch = globalThis.fetch;
const rebound = fetch;
const indexed = globalThis["fetch"];

console.log("typeof fetch:", typeof fetch);
console.log("typeof globalThis.fetch:", typeof globalFetch);
console.log("identity:", fetch === globalFetch, rebound === globalFetch, indexed === globalFetch);
console.log("name length:", globalFetch.name, globalFetch.length);

for (const [label, fn] of [
  ["global", globalFetch],
  ["rebound", rebound],
] as const) {
  try {
    await fn("not a url" as any);
    console.log(`${label} reject:`, false);
  } catch (err) {
    console.log(`${label} reject:`, err instanceof Error);
  }
}
