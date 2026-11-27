// A typed array escapes to a callee that keeps it; code the callee stashed
// later detaches or resizes the buffer between two reads of the escaping
// function, which itself never touches `.buffer`. Every read after that must
// see the new storage (undefined past the end, length 0 once detached), never
// the elements the array had when it escaped.
// Compared byte-for-byte against `node --experimental-strip-types`.

let stash: any = null;
function hold(x: any) { stash = x; }
const holdDyn: any = hold;
function transferStashed() { (stash.buffer as any).transfer(); }
const transferDyn: any = transferStashed;
function cloneTransferStashed() { structuredClone(stash.buffer, { transfer: [stash.buffer] }); }
const cloneTransferDyn: any = cloneTransferStashed;
function shrinkStashed() { (stash.buffer as any).resize(3 * 8); }
const shrinkDyn: any = shrinkStashed;
function growStashed() { (stash.buffer as any).resize(12 * 8); }
const growDyn: any = growStashed;

function show(label: string, v: unknown) { console.log(label + ":", v); }

export function transferMidLoop(): void {
  const a = new Float64Array(8);
  for (let i = 0; i < 8; i++) a[i] = i + 1;
  holdDyn(a);
  let s = 0;
  const seen: unknown[] = [];
  for (let i = 0; i < 8; i++) {
    if (i === 4) transferDyn();
    s += a[i];
    seen.push(a[i]);
  }
  show("transfer mid loop", [s, seen.join("|"), a.length].join(","));
}

export function transferBetweenLoops(n: number): void {
  const a = new Int32Array(8);
  for (let i = 0; i < 8; i++) a[i] = i * 10;
  holdDyn(a);
  let before = 0;
  for (let r = 0; r < n; r++) for (let i = 0; i < 8; i++) before += a[i];
  transferDyn();
  let after = 0;
  for (let r = 0; r < n; r++) for (let i = 0; i < 8; i++) after += a[i];
  show("transfer between loops", [before, after, a.length, a[0]].join(","));
}

export function structuredCloneTransfer(): void {
  const a = new Float32Array(6);
  for (let i = 0; i < 6; i++) a[i] = i + 0.5;
  holdDyn(a);
  let before = 0;
  for (let i = 0; i < 6; i++) before += a[i];
  cloneTransferDyn();
  let after = 0;
  for (let i = 0; i < 6; i++) after += a[i];
  show("structuredClone transfer", [before, after, a.length, a[2]].join(","));
}

export function resizeLengthTracking(): void {
  const a = new Float64Array(new (ArrayBuffer as any)(8 * 8, { maxByteLength: 16 * 8 }));
  for (let i = 0; i < 8; i++) a[i] = i + 1;
  holdDyn(a);
  let before = 0;
  for (let i = 0; i < 8; i++) before += a[i];
  shrinkDyn();
  let shrunk = 0;
  for (let i = 0; i < 8; i++) shrunk += a[i];
  const lenShrunk = a.length;
  growDyn();
  let grown = 0;
  for (let i = 0; i < 12; i++) grown += a[i];
  show("resize", [before, shrunk, lenShrunk, a.length, grown, a[2], a[5]].join(","));
}

transferMidLoop();
transferBetweenLoops(2);
structuredCloneTransfer();
resizeLengthTracking();
