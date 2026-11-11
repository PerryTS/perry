// A `const` RequestInit local mutated after its literal initializer (the shape
// Astro's node adapter uses: `Object.assign(options, { body, duplex })`) must be
// read as it is at the `new Request` call, not as its initializer literal.
const enc = new TextEncoder();
async function* gen() {
  yield enc.encode('{"n":');
  yield enc.encode("1}");
}

let headerCalls = 0;
function makeHeaders() {
  headerCalls++;
  return { "x-a": "1" };
}

const assigned = { method: "POST", headers: makeHeaders() };
Object.assign(assigned, { body: '{"n":1}', duplex: "half" });
const r1 = new Request("http://x/", assigned);
console.log("assign:", r1.method, r1.headers.get("x-a"), JSON.stringify(await r1.text()));

const set: Record<string, unknown> = { method: "PUT" };
set.body = "plain";
const r2 = new Request("http://x/", set);
console.log("set:", r2.method, JSON.stringify(await r2.text()));

const iter = { method: "POST", headers: makeHeaders() };
Object.assign(iter, { body: gen(), duplex: "half" });
const r3 = new Request("http://x/", iter);
console.log("async iterable:", JSON.stringify(await r3.json()));

console.log("header initializers ran:", headerCalls);
