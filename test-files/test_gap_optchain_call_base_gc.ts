// #11619: a captured call result survives collection in the computed key.
declare const gc: undefined | (() => void);
let calls = 0;
function base() { calls++; return { child: { x: 41 } }; }
function key() { if (typeof gc === "function") gc(); return "child"; }
console.log("gc", base()?.[key()]?.x, calls);
let suspended = 0;
async function load() { suspended++; return { x: 43 }; }
const order: string[] = [];
const value = (order.push("before"), (await load())?.x);
console.log("sequence", value, suspended, order.join(","));
let skipped = 0;
async function missing() { return undefined as undefined | { x: number }; }
console.log("skip await key", (await missing())?.[(await Promise.resolve((skipped++, "x")))], skipped);
