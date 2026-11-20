// Gap test for #7548 — `Object.*` operations on an array whose dense storage
// has been REALLOCATED by a capacity-extending write.
//
// `js_array_grow` reallocates header+elements as one allocation and leaves a
// forwarding stub at the old address (issue #233), and the stub's first 8 bytes
// are exactly where `length` and `capacity` live — so they read back as the two
// halves of the forwarding POINTER. The array branches of `Object.*` cast the
// caller's pointer to an `ArrayHeader` without following that chain, so
// `(*arr).length` returned a heap address (~6·10^8) instead of the real length.
//
// Two loops are driven by that length and became bounded-but-unreachable walks
// — one `to_string()` plus a side-table probe per index — which present as a
// hang rather than a wrong answer:
//   * `Object.freeze` / `Object.seal` of any array that has ever grown.
//   * ArraySetLength's shrink walk, reached by the `Set(receiver, "length", …)`
//     tail of an `Array.prototype.splice` that grows a Proxy receiver (the
//     original #7548 report, via test_gap_6908_proxy_array_mutators.ts).
//
// A JS binding keeps the pre-grow address, so every case below hands the stub
// to the entry point under test.
// Compared byte-for-byte against `node --experimental-strip-types`.

// ---- freeze / seal after a capacity-extending push ----
{
  const t: any = [1, 2, 3, 4, 5];
  t.push(6);
  Object.freeze(t);
  console.log("freeze-after-push:", t.length, Object.isFrozen(t), t.join(","));
}
{
  const t: any = [1, 2, 3];
  t.push(4, 5, 6, 7);
  Object.seal(t);
  console.log("seal-after-push:", t.length, Object.isSealed(t), t.join(","));
}
{
  // The freeze walk must record attrs for the array's REAL indices — the whole
  // point of the walk that used to run past 6·10^8. (Whether the element-WRITE
  // path then honors them on a grown array is a separate, pre-existing
  // address-keying inconsistency in the attrs side table, not covered here.)
  const t: any = [1, 2];
  t.push(3);
  Object.freeze(t);
  console.log(
    "freeze-descriptors:",
    JSON.stringify(Object.getOwnPropertyDescriptor(t, "0")),
    JSON.stringify(Object.getOwnPropertyDescriptor(t, "2")),
    Object.getOwnPropertyDescriptor(t, "3") === undefined,
  );
}

// ---- defineProperty("length") after a capacity-extending index define ----
{
  const t: any = [1, 2, 3, 4, 5];
  Object.defineProperty(t, "5", { value: 6, writable: true, enumerable: true, configurable: true });
  Object.defineProperty(t, "length", { value: 6 });
  console.log("define-length-same:", t.length, t.join(","));
}
{
  const t: any = [1, 2, 3];
  Object.defineProperty(t, "3", { value: 4, writable: true, enumerable: true, configurable: true });
  Object.defineProperty(t, "length", { value: 7 });
  console.log("define-length-grow:", t.length);
}
{
  const t: any = [1, 2, 3];
  Object.defineProperty(t, "3", { value: 4, writable: true, enumerable: true, configurable: true });
  Object.defineProperty(t, "length", { value: 2 });
  console.log("define-length-shrink:", t.length, t.join(","));
}
{
  const t: any = [1, 2, 3];
  Object.defineProperty(t, "3", { value: 4, writable: true, enumerable: true, configurable: true });
  const ok = Reflect.defineProperty(t, "length", { value: 4 });
  console.log("reflect-define-length:", ok, t.length);
}

// ---- `length` write through a Proxy receiver after the array grew ----
// This is the exact tail of the #7548 report: `Set(receiver, "length", n)` with
// a Proxy receiver reaches ArraySetLength through [[DefineOwnProperty]].
{
  const t: any = [1, 2, 3, 4, 5];
  const p: any = new Proxy(t, {});
  Reflect.set(t, "5", 5, p);
  const ok = Reflect.set(t, "length", 6, p);
  console.log("proxy-receiver-length:", ok, t.length, t.join(","));
}
{
  const t: any = [1, 2, 3, 4, 5];
  const p: any = new Proxy(t, {});
  const removed = p.splice(1, 2, "a", "b", "c");
  console.log("proxy-splice-grow:", t.join(","), t.length, removed.join(","));
}
{
  const t: any = [1, 2, 3];
  const p: any = new Proxy(t, {});
  const n = p.push(4, 5, 6, 7);
  console.log("proxy-push-grow:", t.join(","), n);
}
{
  const t: any = [1, 2, 3];
  const p: any = new Proxy(t, {});
  const n = p.unshift(0);
  console.log("proxy-unshift-grow:", t.join(","), n);
}

// ---- propertyIsEnumerable / descriptor reads survive the reallocation ----
{
  const t: any = [1, 2, 3];
  t.push(4, 5);
  console.log(
    "enumerable-after-grow:",
    t.propertyIsEnumerable("0"),
    t.propertyIsEnumerable("4"),
    t.propertyIsEnumerable("5"),
  );
}
{
  const t: any = [1, 2, 3];
  t.push(4);
  console.log("keys-after-grow:", Object.keys(t).join(","), JSON.stringify(t));
}
