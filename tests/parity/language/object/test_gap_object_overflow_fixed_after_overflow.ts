// A fixed-slot key written later than an overflow key enumerates after it:
// once an object has an overflow, the overflow is the order authority and
// records the fixed slot at its write position (§3.2, §3.4).
function show(label: string, o: any): void {
  const forin: string[] = [];
  for (const k in o) forin.push(k);
  console.log(label, Object.keys(o).join(","), "|", forin.join(","), "|", JSON.stringify(o));
}
const dyn = ["d1", "d2"];

// A literal's own key deleted, a dynamic key added, then the literal key again.
const a: any = { x: 1, y: 2 };
delete a.x;
a[dyn[0]] = "dyn";
a.x = 3;
show("literal", a);

// A constructor that creates one key, a dynamic key, then a static key later
// (the stream `_eventsCount` / Readable.from hook pattern).
function Emitter(this: any) {
  this._events = {};
}
const e: any = new (Emitter as any)();
e[dyn[1]] = "hook";
e._eventsCount = 0;
e._events = { data: 1 };
show("ctor", e);
const e2: any = new (Emitter as any)();
e2._eventsCount = 0;
e2[dyn[1]] = "hook";
show("ctor2", e2);

// Class instance: declared field, a dynamic key, then the declared field rewritten.
class Box {
  v = 1;
  w = 2;
}
const b: any = new Box();
delete b.v;
b[dyn[0]] = "x";
b.w = 5;
b.v = 7;
show("class", b);
console.log(JSON.stringify(Object.assign({}, b)), JSON.stringify({ ...b }));
