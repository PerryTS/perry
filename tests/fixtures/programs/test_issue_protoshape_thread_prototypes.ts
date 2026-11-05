// Prototype-in-shape across perry/thread agents: every agent mints its own
// shape records, and a record whose identity names a prototype object holds
// a pointer into ITS agent heap, so the spawner's seed must never hand one to
// a worker. Each agent builds function-constructor instances,
// Object.create(proto) and Object.create(null) objects and reads their
// prototypes back; the main thread re-checks its own afterwards.
// perry-only (perry/thread), so a behavioural test: it throws on a mismatch.
import { parallelMap } from "perry/thread";
function P(this: any, v: number) { this.v = v; }
(P as any).prototype.twice = function (this: any) { return this.v * 2; };
const mainProto = { tag: "main" };
const mainObjs: any[] = [];
for (let i = 0; i < 1000; i++) { mainObjs.push(new (P as any)(i)); mainObjs.push(Object.create(mainProto)); }
const res = parallelMap([1, 2, 3, 4, 5, 6, 7, 8], (n: number) => {
  function W(this: any, v: number) { this.v = v; }
  (W as any).prototype.plus = function (this: any) { return this.v + 100; };
  const proto = { tag: "w" + n };
  let ok = 0;
  for (let i = 0; i < 2000; i++) {
    const a: any = new (W as any)(i);
    const b: any = Object.create(proto);
    const c: any = Object.create(null);
    if (a.plus() === i + 100 && a instanceof (W as any) && b.tag === "w" + n && Object.getPrototypeOf(b) === proto && Object.getPrototypeOf(c) === null) ok++;
  }
  return ok;
});
let mok = 0;
for (let i = 0; i < mainObjs.length; i += 2) if (mainObjs[i].twice() === (i / 2) * 2 && mainObjs[i + 1].tag === "main") mok++;
console.log("workers", res.join(","), "main", mok);
if (res.some((n: number) => n !== 2000) || mok !== 1000) throw new Error("prototype lost across agents");
