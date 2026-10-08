// parity-node-argv: --expose-gc
import { Buffer } from "node:buffer";
import * as ns from "./fixtures/byte_scanning_table.ts";
declare function gc(): void;

// Runtime bounds prevent HIR unrolling from masking a missing dirty store.
// No indexed read in these loops: a later write/intrinsic cannot install a
// proof after the callback's invalidation stores have already been emitted.
function writeOnly(b: Uint8Array, cb: (i: number) => void, count: number): void {
  for (let i = 0; i < count; i++) { cb(i); b[i] = 19; }
}
function localWrite(owner: ArrayBuffer, cb: (i: number) => void, count: number): void {
  const b = new Uint8Array(owner, 16, 4);
  for (let i = 0; i < count; i++) { cb(i); b[i] = 19; }
}
function intrinsicOnly(b: Buffer, cb: (i: number) => void, count: number): string {
  let s = 0;
  for (let i = 0; i < count; i++) {
    try { cb(i); s += b.readUInt32LE(i); }
    catch { return s + ":throws:" + i; }
  }
  return String(s);
}
function localIntrinsic(owner: ArrayBuffer, cb: (i: number) => void, count: number): string {
  const b = Buffer.from(owner, 16, 8);
  let s = 0;
  for (let i = 0; i < count; i++) {
    try { cb(i); s += b.readUInt32LE(i); }
    catch { return s + ":throws:" + i; }
  }
  return String(s);
}
// A literal key also exercises the cached i32 write/intrinsic path when
// an unconstrained runtime counter keeps the variable-key path generic.
function writeSlot(b: Uint8Array, cb: (i: number) => void, count: number): void {
  for (let i = 0; i < count; i++) { cb(i); b[1] = i + 19; }
}
function localSlot(owner: ArrayBuffer, cb: (i: number) => void, count: number): void {
  const b = new Uint8Array(owner, 16, 4);
  for (let i = 0; i < count; i++) { cb(i); b[1] = i + 19; }
}
function localIntrinsicSlot(owner: ArrayBuffer, cb: (i: number) => void, count: number): string {
  const b: Buffer = Buffer.from(owner, 16, 8);
  let s = 0;
  for (let i = 0; i < count; i++) {
    try { cb(i); s += b.readUInt32LE(0); }
    catch { return s + ":throws:" + i; }
  }
  return String(s);
}
for (const local of [false, true]) {
  const owner = new ArrayBuffer(4096);
  const b = new Uint8Array(owner, 16, 4);
  b.fill(3);
  let moved: ArrayBuffer | undefined;
  const cb = (i: number) => { if (i === 1) { moved = owner.transfer(); } };
  if (local) localWrite(owner, cb, 4); else writeOnly(b, cb, 4);
  const copy = new Uint8Array(moved!);
  console.log("write-detach", local, b.length, String(b[1]), copy[16], copy[17], copy[18]);
}
for (const local of [false, true]) {
  const owner = new ArrayBuffer(4096);
  new Uint8Array(owner).fill(1);
  const b = Buffer.from(owner, 16, 8);
  const cb = (i: number) => { if (i === 1) { owner.transfer(); } };
  console.log("intrinsic-detach", local, local ? localIntrinsic(owner, cb, 3) : intrinsicOnly(b, cb, 3));
}

for (const local of [false, true]) {
  const owner = new ArrayBuffer(4096);
  const b = new Uint8Array(owner, 16, 4);
  b.fill(3);
  let moved: ArrayBuffer | undefined;
  const cb = (i: number) => { if (i === 1) { moved = owner.transfer(); } };
  if (local) localSlot(owner, cb, 4); else writeSlot(b, cb, 4);
  console.log("write-slot-detach", local, b.length, new Uint8Array(moved!)[17]);
}
const slotOwner = new ArrayBuffer(4096);
new Uint8Array(slotOwner).fill(1);
console.log("intrinsic-slot-detach", localIntrinsicSlot(slotOwner,
  (i) => { if (i === 1) { slotOwner.transfer(); } }, 3));

function scan(b: Uint8Array, cb: (i: number) => void, count: number): string {
  let out = "";
  for (let i = 0; i < count; i++) { cb(i); out += String(b[i]) + ","; }
  return out;
}
const shared = new SharedArrayBuffer(24);
const sab = new Uint8Array(shared);
sab.fill(7);
const sub = sab.subarray(16);
console.log("shared-subarray", scan(sub, (i) => { if (i === 1) { Atomics.store(sub, 2, 23); gc(); } }, 4));
const sharedBuffer = Buffer.from(shared, 16, 4);
console.log("shared-buffer", scan(sharedBuffer, () => {}, 4));

try { (ns as any).table = new Uint8Array([91]); console.log("namespace-write allowed"); }
catch { console.log("namespace-write throws"); }
console.log("namespace-table", ns.table[0]);

let table = new Uint8Array([2, 3, 5, 7]);
function replaceTable(): void { table = new Uint8Array([11, 13, 17, 19]); }
function mutableTable(cb: (i: number) => void, count: number): string {
  let out = "";
  for (let i = 0; i < count; i++) { cb(i); out += String(table[i]) + ","; }
  return out;
}
console.log("reassigned", mutableTable((i) => { if (i === 1) { replaceTable(); gc(); } }, 4));

// Number induction and indexed reads admit the fast clone; length after a
// callback must still observe detachment, including on its first use.
export function fastLength(b: Uint8Array, i: number, end: number, cb: (i: number) => void): string {
  const stop = Math.min(end, end);
  let out = "";
  for (; i < stop; i++) { b[i]; cb(i); out += b.length + ","; }
  return out;
}
const lengthOwner = new ArrayBuffer(4);
console.log("fast-length-detach", fastLength(new Uint8Array(lengthOwner), 0, 4,
  (i) => { if (i === 1) { lengthOwner.transfer(); } }));

// The initializer can return the same view on every iteration. Its callback
// must dirty the previously admitted storage proof before detaching that view.
function initializerAlias(count: number, cb: () => Uint8Array): void {
    for (let i = 0; i < count; i++) {
        const view: Uint8Array = cb();
        view[0] = i + 19;
        console.log("initializer alias", i, view.length, view[0]);
    }
}
const initializerOwner = new ArrayBuffer(4096);
const initializerView = new Uint8Array(initializerOwner, 16, 4);
let initializerCalls = 0;
initializerAlias(3, () => {
    if (initializerCalls++ === 1) initializerOwner.transfer();
    return initializerView;
});
