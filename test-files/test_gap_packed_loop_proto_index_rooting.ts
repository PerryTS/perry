// `arr[i]` inside the packed-numeric fast clone is a proven Number, so its
// `const` binding shades no GC root. `arr[i ± c]` is NOT: the index can leave
// the array, and an out-of-bounds element read consults the prototype chain,
// where an installed index property is a genuine heap reference that must stay
// rooted. This fixture puts an object there, has an offset loop READ it, and
// retains it across collections — so a missed shading shows up as a dangling
// reference instead of as luck.

const protoHeld: any = { tag: "proto-object", payload: [11, 22, 33] };
Object.defineProperty(Array.prototype, 7, {
  value: protoHeld,
  writable: true,
  enumerable: false,
  configurable: true,
});

// Counter read: always in bounds, never reaches the prototype.
function sumCounter(a: number[]): number {
  let s = 0;
  for (let i = 0; i < a.length; i++) {
    const v = a[i];
    s += v;
  }
  return s;
}

// Offset read: a[i + 3] over a length-5 array touches 3, 4, 5, 6, 7 — the last
// of which is the prototype's object.
function collectOffset(a: number[], out: any[]): number {
  let n = 0;
  for (let i = 0; i < a.length; i++) {
    const w = a[i + 3];
    out.push(w);
    n++;
  }
  return n;
}

const five: number[] = [1.5, 2.5, 3.5, 4.5, 5.5];
console.log("counter", sumCounter(five));

const captured: any[] = [];
console.log("offset-count", collectOffset(five, captured));
console.log("offset-kinds", captured.map((x) => typeof x).join(","));
console.log("offset-values", JSON.stringify(captured.slice(0, 2)));
console.log("captured-is-proto", captured[4] === protoHeld);

// Churn the nursery so the retained capture crosses collections.
let churn: any[] = [];
for (let round = 0; round < 60; round++) {
  const block: any[] = [];
  for (let k = 0; k < 200; k++) block.push({ k, s: "fill-" + k, arr: [k, k + 1] });
  churn.push(block);
  if (churn.length > 4) churn = churn.slice(-2);
  // Re-run both loops while the heap is moving.
  sumCounter(five);
  collectOffset(five, captured);
}

// The prototype object captured before all that churn must still be intact.
const held = captured[4];
console.log("held-tag", held.tag);
console.log("held-payload", JSON.stringify(held.payload));
console.log("held-identity", held === protoHeld, protoHeld.tag);
console.log("captured-len", captured.length);
console.log("last-capture-ok", captured[captured.length - 1] === protoHeld);

// The counter loop's answer is unchanged by the polluted prototype.
console.log("counter-again", sumCounter(five));

// And a length-8 array reads its OWN element 7, not the prototype's.
const eight: number[] = [1, 2, 3, 4, 5, 6, 7, 8];
const ownSeven: any[] = [];
collectOffset(eight.slice(0, 5), ownSeven);
console.log("short-slice-sees-proto", ownSeven[4] === protoHeld);
console.log("full-eight-index7", eight[7]);
console.log("sum-eight", sumCounter(eight));

delete (Array.prototype as any)[7];
console.log("after-delete", five[7], eight[7]);
