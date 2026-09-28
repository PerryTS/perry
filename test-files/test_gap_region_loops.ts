// Step 4b (#10884): loop and body regions. Every case is a way the facts a
// region's guard established can stop holding mid-loop, or a receiver the
// guard must refuse. Output must be byte-identical to node.

function out(label: string, v: any): void {
  console.log(label + ": " + (typeof v === "string" ? v : JSON.stringify(v)));
}

// 1. A call in the body installs a getter on the receiver's own key.
function getterMidLoop(o: any, n: number): number {
  let h = 0;
  for (let i = 0; i < n; i++) {
    h += o.a;
    if (i === 2) {
      Object.defineProperty(o, "a", { get() { return 100; }, configurable: true });
    }
    o.b = i;
  }
  return h + o.b;
}
out("getterMidLoop", getterMidLoop({ a: 1, b: 0 }, 6));

// 2. A call deletes the key the region reads.
function deleteMidLoop(o: any, n: number): string {
  let s = "";
  for (let i = 0; i < n; i++) {
    s += String(o.a) + ",";
    if (i === 1) delete o.a;
  }
  return s;
}
out("deleteMidLoop", deleteMidLoop({ a: 7, b: 2 }, 4));

// 3. valueOf of a READ value reshapes the receiver inside a fact tree.
function valueOfInTree(o: any, n: number): number {
  o.a = { valueOf() { o.c = 1000; delete o.b; return 1; } };
  let h = 0;
  for (let i = 0; i < n; i++) {
    h += o.a + o.b + o.c;
    o.a = i;
  }
  return h;
}
out("valueOfInTree", valueOfInTree({ a: 1, b: 2, c: 3 }, 4));

// 4. Key added in the first iteration only.
function addFirst(o: any, n: number): number {
  let h = 0;
  for (let i = 0; i < n; i++) {
    o.z = i;
    h += o.z + o.a;
  }
  return h;
}
out("addFirst", addFirst({ a: 5 }, 5));

// 5. Different shapes on different entries of the same loop.
function sum2(o: any, n: number): number {
  let h = 0;
  for (let i = 0; i < n; i++) {
    o.d = i;
    h += o.a + o.b;
  }
  return h;
}
out("poly", [sum2({ a: 1, b: 2, d: 0 }, 3), sum2({ b: 20, a: 10, d: 0 }, 3), sum2({ x: 0, a: 100, b: 200, d: 0 }, 3), sum2({ a: 1, b: 2, d: 0 }, 3)]);

// 6. Frozen in the middle: sloppy store silently ignored; strict throws.
function freezeMid(o: any, n: number): number {
  let h = 0;
  for (let i = 0; i < n; i++) {
    o.a = i;
    h += o.a;
    if (i === 2) Object.freeze(o);
  }
  return h;
}
out("freezeMid", freezeMid({ a: 0, b: 1 }, 6));
function strictFreeze(o: any): string {
  "use strict";
  try {
    for (let i = 0; i < 5; i++) {
      o.a = i;
      if (i === 1) Object.freeze(o);
    }
    return "no throw";
  } catch (e: any) {
    return "threw " + (e instanceof TypeError) + " a=" + o.a;
  }
}
out("strictFreeze", strictFreeze({ a: 0 }));

// 7. Pointer-valued stores under allocation pressure (moving GC + barriers).
function pointerStores(o: any, n: number): number {
  let total = 0;
  for (let i = 0; i < n; i++) {
    o.a = { v: i, pad: [i, i + 1, i + 2] };
    o.b = i;
    total += o.a.v + o.a.pad.length + o.b;
  }
  return total;
}
out("pointerStores", pointerStores({ a: null, b: 0 }, 20000));

// 8. Receivers the store guard must refuse: a typed-array prototype-like
// class-less object and an Array subclass carrying elements.
class MyArr extends Array<number> {
  tag: number = 0;
}
function arrSub(n: number): string {
  const a: any = new MyArr();
  a.push(1, 2, 3);
  let s = 0;
  for (let i = 0; i < n; i++) {
    a.tag = i;
    s += a.tag + a[0];
  }
  return s + ":" + a.length + ":" + a.join("-");
}
out("arrSub", arrSub(5));

// 9. `var` re-declared in the loop body, read after the loop.
function varInBody(o: any, n: number): number {
  for (let i = 0; i < n; i++) {
    var x = o.a + i;
    o.b = x;
  }
  // @ts-ignore
  return x + o.b;
}
out("varInBody", varInBody({ a: 3, b: 0 }, 4));

// 10. Body region whose receiver changes shape per iteration.
function bodyPoly(n: number): number {
  const objs: any[] = [{ a: 1, d: 0 }, { d: 0, a: 2 }, { a: 3, x: 1, d: 0 }, { a: 4, d: 0 }];
  let h = 0;
  for (let k = 0; k < n; k++) {
    const o = objs[k & 3];
    o.d = k;
    h += o.a + o.d;
  }
  return h;
}
out("bodyPoly", bodyPoly(12));

// 11. continue / break inside the body.
function contBreak(o: any, n: number): number {
  let h = 0;
  for (let i = 0; i < n; i++) {
    if (i % 3 === 0) {
      o.a = o.a + 1;
      continue;
    }
    if (i > 10) break;
    h += o.a + o.b;
  }
  return h;
}
out("contBreak", contBreak({ a: 1, b: 2 }, 20));

// 12. Nested loops over the same receiver.
function nested(o: any): number {
  let h = 0;
  for (let i = 0; i < 4; i++) {
    for (let j = 0; j < 3; j++) {
      o.c = i * j;
      h += o.a + o.c;
    }
    h += o.b;
  }
  return h;
}
out("nested", nested({ a: 1, b: 10, c: 0 }));

// 13. `this` in a method called on different receivers.
class P {
  a = 1;
  b = 2;
  d = 0;
  run(n: number): number {
    let h = 0;
    for (let i = 0; i < n; i++) {
      this.d = i;
      h += this.a + this.b;
    }
    return h;
  }
}
class Q extends P {
  q = 5;
}
const pq: P[] = [new P(), new Q(), new P()];
out("thisPoly", pq.map((p) => p.run(3)));

// 14. An exception thrown from inside the F-body, caught outside the loop.
function throwInBody(o: any): string {
  try {
    for (let i = 0; i < 5; i++) {
      o.a = i;
      if (i === 3) throw new Error("x" + o.a);
    }
  } catch (e: any) {
    return e.message + ":" + o.a;
  }
  return "none";
}
out("throwInBody", throwInBody({ a: 0 }));

// 15. while and do-while.
function whileLoops(o: any): number {
  let i = 0;
  let h = 0;
  while (i < 5) {
    o.a = i;
    h += o.a + o.b;
    i++;
  }
  do {
    o.b = i;
    h += o.b;
    i--;
  } while (i > 0);
  return h;
}
out("whileLoops", whileLoops({ a: 0, b: 1 }));

// 16. setPrototypeOf mid-loop does not affect own keys; an own key shadowed later does.
function protoMid(o: any, n: number): number {
  let h = 0;
  for (let i = 0; i < n; i++) {
    h += o.a;
    if (i === 1) Object.setPrototypeOf(o, { a: 999, z: 1 });
  }
  return h;
}
out("protoMid", protoMid({ a: 2 }, 4));

// 17. Dictionary-mode receiver (many deletes/adds).
function dict(o: any, n: number): number {
  for (let i = 0; i < 40; i++) o["k" + i] = i;
  for (let i = 0; i < 30; i++) delete o["k" + i];
  o.a = 5;
  o.b = 6;
  let h = 0;
  for (let i = 0; i < n; i++) {
    o.b = i;
    h += o.a + o.b;
  }
  return h;
}
out("dict", dict({}, 5));

// 18. A Proxy receiver.
function proxyRecv(n: number): string {
  const log: string[] = [];
  const p: any = new Proxy({ a: 1, b: 2 } as any, {
    get(t, k) { log.push("g" + String(k)); return t[k]; },
    set(t, k, v) { log.push("s" + String(k)); t[k] = v; return true; },
  });
  let h = 0;
  for (let i = 0; i < n; i++) {
    p.b = i;
    h += p.a + p.b;
  }
  return h + ":" + log.join("");
}
out("proxyRecv", proxyRecv(3));

// 19. A string value in the keys the tree reads (concatenation, not addition).
function strTree(o: any, n: number): string {
  let s: any = "";
  for (let i = 0; i < n; i++) {
    o.d = i;
    s = s + o.a + o.b;
  }
  return s;
}
out("strTree", strTree({ a: "x", b: 1, d: 0 }, 3));

// 20. Receiver reassigned inside the loop (must not be a region receiver).
function reassigned(n: number): number {
  let o: any = { a: 1, b: 2 };
  let h = 0;
  for (let i = 0; i < n; i++) {
    h += o.a + o.b;
    if (i === 1) o = { b: 20, a: 10 };
  }
  return h;
}
out("reassigned", reassigned(4));

// 21. Getter on the prototype chain for a key that is later made own.
function inheritedThenOwn(n: number): number {
  const proto = { get a() { return 50; } };
  const o: any = Object.create(proto);
  o.b = 1;
  let h = 0;
  for (let i = 0; i < n; i++) {
    h += o.a + o.b;
    if (i === 1) Object.defineProperty(o, "a", { value: 3, writable: true, enumerable: true, configurable: true });
  }
  return h;
}
out("inheritedThenOwn", inheritedThenOwn(4));

// 22. Non-writable own data property: sloppy store ignored.
function nonWritable(o: any, n: number): number {
  Object.defineProperty(o, "a", { value: 1, writable: false });
  let h = 0;
  for (let i = 0; i < n; i++) {
    o.a = 100;
    o.b = i;
    h += o.a + o.b;
  }
  return h;
}
out("nonWritable", nonWritable({ a: 1, b: 0 }, 4));

// 23. Old receiver, young value, then a collection before the read: the
// store's remembered-set/marking obligations are what keep the value alive.
function barrierOldToYoung(o: any, n: number): number {
  const keep: any[] = [];
  for (let r = 0; r < 200000; r++) keep.push({ r });
  let s = 0;
  for (let i = 0; i < n; i++) {
    o.a = { v: i, w: [i] };
    o.b = i;
    const junk: any[] = [];
    for (let j = 0; j < 400; j++) junk.push({ j, k: [j] });
    s += o.a.v + o.a.w[0] + junk.length;
  }
  return s + keep.length;
}
out("barrierOldToYoung", barrierOldToYoung({ a: null, b: 0 }, 300));

// 24. A module `const` receiver read by a loop that runs before the const is
// initialised, with zero iterations: the guard must not hoist a TDZ read.
function early(n: number): number {
  let h = 0;
  for (let i = 0; i < n; i++) {
    LATE.b = i;
    h += LATE.a + LATE.b;
  }
  return h;
}
out("earlyZero", early(0));
const LATE: any = { a: 1, b: 0 };
out("lateRun", early(3));

// 25. A derived constructor looping over `this` after super().
class Base25 {
  a = 1;
  b = 0;
}
class Derived25 extends Base25 {
  c: number;
  constructor(n: number) {
    super();
    this.c = 0;
    for (let i = 0; i < n; i++) {
      this.b = i;
      this.c = this.c + this.a + this.b;
    }
  }
}
out("derivedCtor", new Derived25(4).c);

// 26. Module const receiver, polymorphic across calls via a mutable field.
const MC: any = { a: 2, b: 3, d: 0 };
function modconstLoop(n: number): number {
  let h = 0;
  for (let i = 0; i < n; i++) {
    MC.d = i;
    h += MC.a + MC.b + MC.d;
  }
  return h;
}
out("modconst1", modconstLoop(4));
delete MC.b;
MC.b = 30;
out("modconst2", modconstLoop(4));

// 27-28. A USER call reshapes the receiver (the planner must treat any call
// as able to run JS; the IR verifier backs it).
function mutDelete(o: any): void {
  delete o.a;
}
function mutGetter(o: any): void {
  Object.defineProperty(o, "b", { get() { return 77; }, configurable: true });
}
function userCallReshape(o: any, n: number): string {
  let s = "";
  for (let i = 0; i < n; i++) {
    o.c = i;
    s += String(o.a) + "/" + String(o.b) + ";";
    if (i === 1) mutDelete(o);
    if (i === 2) mutGetter(o);
  }
  return s;
}
out("userCallReshape", userCallReshape({ a: 1, b: 2, c: 0 }, 5));
function mutGetterA(o: any): void {
  Object.defineProperty(o, "a", { get() { return 1000; }, configurable: true });
}
// The mutator arrives as a PARAMETER so no inliner can turn the call into
// the builtin it wraps: the planner sees a plain call.
function userCallReshape2(o: any, n: number, f: (o: any) => void): number {
  let h = 0;
  for (let i = 0; i < n; i++) {
    h += o.a + o.b;
    if (i === 1) f(o);
  }
  return h;
}
out("userCallReshape2", userCallReshape2({ a: 1, b: 2 }, 4, mutGetterA));

// 29. The ONLY thing between iterations that can run JS is a fact tree's
// generic arm (valueOf of a read value), and it deletes a key the tree reads.
let first = true;
function treeOnlyKill(o: any, n: number): number {
  let h = 0;
  for (let i = 0; i < n; i++) {
    h += o.a + o.b + o.c;
  }
  return h;
}
const tok: any = { a: 0, b: 2, c: 3 };
tok.a = { valueOf() { tok.a = 1; Object.defineProperty(tok, "c", { get() { return 500; }, configurable: true }); return 1; } };
out("treeOnlyKill", treeOnlyKill(tok, 4));

// 30. A class-less receiver whose own slots are not plain data (URL).
function urlStore(u: any, n: number): string {
  let h = 0;
  for (let i = 0; i < n; i++) {
    u.hash = "#" + i;
    h += u.hash.length;
  }
  return h + " " + u.href;
}
out("urlStore", urlStore(new URL("http://example.com/p"), 3));

// 31. Stale facts would be observable OUTSIDE a verified fact tree: an
// identity compare of a read against what a getter returns. The only thing in
// the loop that can reshape the receiver is a user call through a parameter.
function staleCompare(o: any, n: number, f: (o: any) => void): number {
  let c = 0;
  for (let i = 0; i < n; i++) {
    if (o.a === 1000) c++;
    if (i === 1) f(o);
  }
  return c;
}
out("staleCompare", staleCompare({ a: 1, b: 2 }, 5, mutGetterA));

// 32. The only JS between iterations is a fact tree's generic arm, and what it
// installs is observed by a compare in the NEXT iteration (the dirty flag).
function dirtyCompare(o: any, n: number): number {
  let h = 0;
  let c = 0;
  for (let i = 0; i < n; i++) {
    if (o.c === 500) c++;
    h += o.a + o.b;
  }
  return h * 100 + c;
}
const dc: any = { a: 0, b: 2, c: 3 };
dc.a = { valueOf() { dc.a = 1; Object.defineProperty(dc, "c", { get() { return 500; }, configurable: true }); return 1; } };
out("dirtyCompare", dirtyCompare(dc, 4));

// 33. Old receiver, young POINTER copied between two of its own keys with
// nothing that can run JS in between (a bare load feeding a bare store), then
// a collection before the value is read back.
function barrierBareCopy(o: any, n: number): number {
  const keep: any[] = [];
  for (let r = 0; r < 200000; r++) keep.push({ r });
  let s = 0;
  for (let i = 0; i < n; i++) {
    o.a = o.b;
    o.b = { v: i + 1, w: [i] };
    const junk: any[] = [];
    for (let j = 0; j < 300; j++) junk.push({ j, k: [j] });
    s += o.a.v + o.a.w.length + junk.length;
  }
  return s + keep.length;
}
out("barrierBareCopy", barrierBareCopy({ a: null, b: { v: 0, w: [] } }, 200));

// 34. Stores to an object that is some other object's PROTOTYPE: an
// inherited read through the child must see every write.
function protoStores(p: any, c: any, n: number): number {
  let h = 0;
  for (let i = 0; i < n; i++) {
    p.x = i;
    p.y = i * 2;
    h += c.x + c.y;
  }
  return h;
}
const PP: any = { x: 0, y: 0 };
const CC: any = Object.create(PP);
out("protoStores", protoStores(PP, CC, 6));

// 35. A class-less receiver whose own keys are not plain data (URL): stores
// with a numeric right-hand side, nothing else in the loop.
function urlPort(u: any, n: number): string {
  for (let i = 0; i < n; i++) {
    u.port = 8000 + i;
  }
  return u.port + " " + u.href + " " + u.host;
}
out("urlPort", urlPort(new URL("http://example.com:81/p"), 3));
function urlPortRead(u: any, n: number): number {
  let h = 0;
  for (let i = 0; i < n; i++) {
    u.port = 9000 + i;
    h += u.port.length;
  }
  return h;
}
out("urlPortRead", urlPortRead(new URL("http://example.com:81/p"), 3));

// 36. A YOUNG receiver read, then an allocation that can collect (but cannot
// run JS, so the facts survive), then read again: the second read must derive
// the receiver's address afresh, because the collection may have moved it.
function allocBetween(o: any, n: number): number {
  const keep: any[] = [];
  let h = 0;
  for (let i = 0; i < n; i++) {
    const x = o.a;
    keep.push({ i, pad: [i, i] });
    const y = o.b;
    h += x.v + y.v;
  }
  return h + keep.length;
}
out("allocBetween", allocBetween({ a: { v: 1 }, b: { v: 2 } }, 30000));

// 37. SPILL-located keys (an Object.create receiver grows past its inline
// slots): reads served through the spill buffer, stores to an inline key, a
// young receiver under allocation pressure so the buffer itself moves.
const SPROTO: any = { p: 100 };
function mkSpill(base: number): any {
  const t: any = Object.create(SPROTO);
  t.a = base + 1; t.b = base + 2; t.c = base + 3; t.e = base + 4; t.d = base + 5;
  return t;
}
function spillRead(o: any, n: number): number {
  let h = 0;
  for (let i = 0; i < n; i++) {
    h += o.a + o.c + o.e + o.d;
  }
  return h;
}
out("spillRead", spillRead(mkSpill(10), 50));
function spillStoreInline(o: any, n: number): number {
  let h = 0;
  for (let i = 0; i < n; i++) {
    o.a = i;
    h += o.a + o.e;
  }
  return h;
}
out("spillStoreInline", spillStoreInline(mkSpill(20), 40));
function spillStoreSpill(o: any, n: number): number {
  let h = 0;
  for (let i = 0; i < n; i++) {
    o.d = i;
    h += o.d + o.e;
  }
  return h;
}
out("spillStoreSpill", spillStoreSpill(mkSpill(30), 40));
function spillBody(n: number): number {
  const objs: any[] = [];
  for (let i = 0; i < 8; i++) objs.push(mkSpill(i * 10));
  let h = 0;
  for (let k = 0; k < n; k++) {
    const o = objs[k & 7];
    const x = o.e;
    const y = o.d;
    h += x + y + o.a;
  }
  return h;
}
out("spillBody", spillBody(64));
function spillMoving(n: number): number {
  let h = 0;
  for (let r = 0; r < n; r++) {
    const o = mkSpill(r);
    const junk: any[] = [];
    for (let j = 0; j < 50; j++) junk.push({ j });
    h += spillRead(o, 3) + junk.length;
  }
  return h;
}
out("spillMoving", spillMoving(3000));
function spillThenReshape(o: any, n: number, f: (o: any) => void): number {
  let h = 0;
  for (let i = 0; i < n; i++) {
    h += o.e + o.d;
    if (i === 2) f(o);
  }
  return h;
}
out("spillThenReshape", spillThenReshape(mkSpill(40), 6, (o: any) => { delete o.e; o.e = 1000; }));
