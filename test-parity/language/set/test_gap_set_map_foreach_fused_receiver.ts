// `.forEach` on an unknown-typed receiver is statically fused to the ARRAY
// forEach entry point. When the receiver is actually a native Set or Map —
// e.g. a collection stored on an object and read back as a property — the
// fused call treated the SetHeader as an ArrayHeader, feeding hash-table
// internals to the callback as elements and segfaulting on the first property
// read. react-server-dom hits exactly this: `request.abortableTasks` is a Set
// it iterates via `.forEach`, reading `.status` off each task — this crashed
// every Next.js App Router dynamic route once the RSC flight started flowing
// (#5989).
//
// `forEach` is the only method name the fused array methods share with
// Set/Map, so the runtime reroute (mirroring the existing typed-array reroute)
// covers the hazard class.
//
// Validated byte-for-byte against `node --experimental-strip-types`.

// (1) the flight shape: Set stored on an object, read back, forEach'd
const req: any = { abortableTasks: new Set() };
req.abortableTasks.add({ status: 10 });
req.abortableTasks.add({ status: 11 });
req.abortableTasks.add({ status: 12 });
const got: any[] = [];
req.abortableTasks.forEach((t: any) => got.push(t.status));
console.log(JSON.stringify(got), req.abortableTasks.size);

// (2) Map stored on an object — callback receives (value, key, map)
const holder: any = { cache: new Map() };
holder.cache.set("a", { n: 1 });
holder.cache.set("b", { n: 2 });
const pairs: string[] = [];
holder.cache.forEach((v: any, k: any) => pairs.push(`${k}=${v.n}`));
console.log(pairs.join(","));

// (3) Set forEach argument order: (value, valueAgain, set)
const s: any = { s: new Set(["x"]) };
s.s.forEach((v: any, v2: any, theSet: any) =>
  console.log(v === v2, theSet.has("x"), theSet.size),
);

// (4) plain arrays through the same fused path stay correct
const arrHolder: any = { list: [7, 8] };
const items: string[] = [];
arrHolder.list.forEach((v: any, i: any, a: any) => items.push(`${i}:${v}:${a.length}`));
console.log(items.join(","));

// (5) delete during Set.forEach (React deletes tasks while sweeping): the
// backing vector compacts on delete, so naive index advancement skipped the
// shifted-in next entry.
const req2: any = { tasks: new Set() };
const t1 = { id: 1 };
const t2 = { id: 2 };
req2.tasks.add(t1);
req2.tasks.add(t2);
const seen: number[] = [];
req2.tasks.forEach((t: any) => {
  seen.push(t.id);
  req2.tasks.delete(t);
});
console.log(JSON.stringify(seen), req2.tasks.size);

// (6) delete during Map.forEach — same compaction hazard
const m6: any = { m: new Map() };
m6.m.set("a", 1);
m6.m.set("b", 2);
m6.m.set("c", 3);
const seen6: string[] = [];
m6.m.forEach((v: any, k: any) => {
  seen6.push(`${k}:${v}`);
  m6.m.delete(k);
});
console.log(JSON.stringify(seen6), m6.m.size);

// (7) delete an EARLIER entry during iteration (must not skip or re-visit)
const s7 = new Set(["p", "q", "r"]);
const seen7: string[] = [];
s7.forEach((v: any) => {
  seen7.push(v);
  if (v === "q") s7.delete("p");
});
console.log(JSON.stringify(seen7), s7.size);
