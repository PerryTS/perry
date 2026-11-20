// #5989: the Promise resolution procedure must read `then` via `Get(resolution,
// "then")`, which for a Proxy resolution value invokes the Proxy's `get` trap.
//
// Perry's `js_dynamic_object_get_property` (the thenable check behind
// `Promise.resolve`/`await`) lacked a Proxy branch, so a Proxy in the proxy id
// band fell into the generic handle dispatch and its `get` trap never fired —
// only direct `proxy.then` / `typeof proxy.then` (which route through the
// codegen property-get) triggered it. React's RSC client-module references are
// exactly such trap-bearing Proxies: their `then` trap lazily marks the module
// as an async ESM export, and the Flight serializer awaits/resolves them, so
// the async flag (the trailing `,1` in `I[id,chunks,name,1]`) was dropped.

function trapProxy(log: string[]) {
  return new Proxy(
    { v: 42 },
    {
      get(t: any, key) {
        if (typeof key === "string") log.push("get:" + key);
        return t[key];
      },
    },
  );
}

async function main() {
  // Promise.resolve must Get(resolution, "then").
  const l1: string[] = [];
  Promise.resolve(trapProxy(l1));
  await Promise.resolve();
  console.log("Promise.resolve reads then:", l1.includes("get:then"));

  // await must Get(value, "then").
  const l2: string[] = [];
  await trapProxy(l2);
  console.log("await reads then:", l2.includes("get:then"));

  // A resolve() inside a new Promise must Get(resolution, "then").
  const l3: string[] = [];
  await new Promise<void>((res) => res(trapProxy(l3) as any));
  console.log("resolve() reads then:", l3.includes("get:then"));

  // Promise.all element resolution reads then on each.
  const l4: string[] = [];
  await Promise.all([trapProxy(l4)]);
  console.log("Promise.all reads then:", l4.includes("get:then"));

  // A thenable Proxy actually assimilates: its `then` is invoked and the
  // resolved value flows through.
  const thenable = new Proxy(
    {},
    {
      get(_t, key) {
        if (key === "then") {
          return (resolve: (v: string) => void) => resolve("assimilated");
        }
        return undefined;
      },
    },
  );
  const result = await (thenable as any);
  console.log("thenable Proxy resolves to:", result);
}

main();
