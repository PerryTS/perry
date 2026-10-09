// parity-node-argv: --expose-gc
"use strict";
export {};
declare function gc(): void;

const map: any = new WeakMap();
const refs: any[] = [];
function deadEntries(): void {
  const direct = {};
  map.set(direct, direct);
  refs.push(new WeakRef(direct));
  const indirect = {};
  map.set(indirect, { back: indirect });
  refs.push(new WeakRef(indirect));
  refs.push(new WeakRef({ control: true }));
}
deadEntries();
const live = {};
map.set(live, { answer: 73 });
const first: any = new WeakMap();
const second: any = new WeakMap();
const chainStart = {};
function linkChain(): any {
  const chainMiddle = {};
  first.set(chainStart, chainMiddle);
  second.set(chainMiddle, { answer: 79 });
  return new WeakRef(second.get(chainMiddle));
}
const end: any = linkChain();
// End the job that allocated WeakRefs so their kept-alive guarantee expires.
setImmediate(() => {
  gc();
  gc();
  gc();
  console.log("control dead", refs[2].deref() === undefined);
  console.log("direct dead", refs[0].deref() === undefined);
  console.log("indirect dead", refs[1].deref() === undefined);
  console.log("live value", map.get(live).answer);
  console.log("fixed point", second.get(first.get(chainStart)).answer, end.deref().answer);
  const set: any = new WeakSet();
  set.add(live);
  console.log("brands", map.has(live), set.has(live));
});
