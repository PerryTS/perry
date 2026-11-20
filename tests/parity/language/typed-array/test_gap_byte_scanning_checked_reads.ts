// parity-node-argv: --expose-gc
import { Buffer } from "node:buffer";
declare function gc(): void;
// Byte owners and views use the same checked element read for every key.
const table = new Uint8Array([3, 5, 7, 11, 13, 17, 19, 23]);
function lookup(bytes: Uint8Array, key: any): any { return bytes[key]; }
function constantLookup(key: any): any { return table[key]; }
function bufferLookup(bytes: Buffer, key: any): any { return bytes[key]; }
const byteBuffer = Buffer.from([3, 5, 7, 11]);
const byteSlice = byteBuffer.subarray(1, 3);
console.log("buffer-view", bufferLookup(byteSlice, 0), bufferLookup(byteSlice, 1), String(bufferLookup(byteSlice, 2)), String(bufferLookup(byteSlice, 0.5)), String(bufferLookup(byteSlice, "01")));
for (const key of [0, -0, 1, 7, 8, -1, 0.5, NaN, Infinity, -Infinity, 2147483648, 4294967294, '1', '01', '-0', 'length']) {
  console.log('key', String(key), String(lookup(table, key)), String(constantLookup(key)));
}
console.log('wrong', lookup({ 0: 73 } as any, 0), lookup('abc' as any, 0));
for (const receiver of [null, undefined, 42, true]) {
  try { console.log('scalar', String(receiver), String(lookup(receiver as any, 0))); }
  catch { console.log('scalar', String(receiver), 'throws'); }
}

function walk(bytes: Uint8Array, start: number, stop: number, change: () => void): string {
  let output = '';
  for (; start < stop; start++) {
    if (start === 2) change();
    output += String(bytes[start]) + ',';
  }
  return output;
}
const fixedBacking = new ArrayBuffer(12);
const fixed = new Uint8Array(fixedBacking, 2, 8);
fixed.set(table);
console.log('view', walk(fixed, 0, 8, () => {}));
console.log('fraction', walk(fixed, 0.5, 4, () => {}));
console.log('string-index', walk(fixed, '1' as any, 4, () => {}));
console.log('bigint-index', walk(fixed, 1n as any, 4, () => {}));
const coercions: string[] = [];
const objectIndex = { valueOf() { coercions.push('valueOf'); return 1; }, toString() { coercions.push('toString'); return '1'; } };
console.log('object-index', walk(fixed, objectIndex as any, 3, () => {}), coercions.join('/'));
console.log('large-index', walk(fixed, 2147483647, 2147483650, () => {}));
console.log('uint32-edge', walk(fixed, 4294967294, 4294967297, () => {}));
console.log('nonfinite', walk(fixed, Infinity, 8, () => {}), walk(fixed, NaN, 8, () => {}));
console.log('negative', walk(fixed, -1, 3, () => {}));
console.log('detach', walk(fixed, 0, 8, () => { fixedBacking.transfer(); }));

const backing = new ArrayBuffer(8, { maxByteLength: 16 });
const tracking = new Uint8Array(backing);
tracking.set(table);
console.log('shrink', walk(tracking, 0, 8, () => { backing.resize(4); }));
console.log('grow', walk(tracking, 0, 10, () => { backing.resize(12); tracking[8] = 29; }));
const fixedResizable = new Uint8Array(backing, 2, 4);
console.log('fixed-shrink', walk(fixedResizable, 0, 6, () => { backing.resize(3); }));
backing.resize(12);
const coercingKey = { toString() { backing.resize(4); return '3'; } };
console.log('key-resize', lookup(tracking, coercingKey), tracking.length);
console.log('restore', lookup(tracking, 4), lookup(tracking, 0.5));

function constantWalk(change: () => void): string {
  let out = '';
  for (let i = 0; i < 4; i++) {
    if (i === 2) change();
    out += String(table[i]) + ',';
  }
  return out;
}
console.log('constant-contents', constantWalk(() => { table[2] = 31; }));
console.log('constant-detach', constantWalk(() => { table.buffer.transfer(); }));

function skip(bytes: Uint8Array, i: number): any {
  while (bytes[i] === 32) i++;
  return i;
}
const spaces = new Uint8Array([32,32,33]);
for (const start of [0, 0.5, -1, NaN, '0', '2', 0n]) {
  const end = skip(spaces, start as any);
  console.log('skip', String(start), String(end), typeof end);
}

function makeOwnerView(): Uint8Array {
  const owner = new ArrayBuffer(80);
  const view = new Uint8Array(owner, 16, 64);
  for (let i = 0; i < 64; i++) view[i] = i + 1;
  return view;
}
function collectScan(view: Uint8Array, i: number): number {
  let sum = 0;
  for (; i < 64; i++) {
    if ((i & 7) === 0) gc();
    sum += view[i];
  }
  return sum;
}
console.log('owner-gc', collectScan(makeOwnerView(), 0));
const gcTable = new Uint8Array(64);
for (let i = 0; i < 64; i++) gcTable[i] = i + 1;
function collectGlobal(): number {
  let sum = 0;
  for (let i = 0; i < 64; i++) {
    if ((i & 7) === 0) gc();
    sum += gcTable[i];
  }
  return sum;
}
console.log('global-gc', collectGlobal());

function liveWalk(bytes: Uint8Array, i: number, change: () => void): string {
  let out = '';
  for (; i < bytes.length; i++) {
    if (i === 2) change();
    out += String(bytes[i]) + ',';
  }
  return out;
}
const liveBacking = new ArrayBuffer(8);
const liveView = new Uint8Array(liveBacking);
liveView.set([3,5,7,11]);
console.log('live-detach', liveWalk(liveView, 0, () => { liveBacking.transfer(); }));
const shadowLength = new Uint8Array([3,5,7,11]);
let lengthReads = 0;
Object.defineProperty(shadowLength, 'length', { get() { lengthReads++; return lengthReads <= 3 ? 4 : 0; } });
console.log('length-getter', liveWalk(shadowLength, 0, () => {}), lengthReads);
console.log('live-wrong', liveWalk({length:3,0:'a',1:'b',2:'c'} as any, 0, () => {}));
console.log('live-string', liveWalk('abc' as any, 0, () => {}));

function truthScan(bytes: Uint8Array, i: number): number {
  let n = 0;
  for (; i < 3; i++) if (bytes[i]) n++;
  return n;
}
function plusScan(bytes: Uint8Array, i: number): string {
  let out = '';
  for (; i < 3; i++) out += String(bytes[i] + 1) + ',';
  return out;
}
const lyingBytes = {0:'a', 1:true, 2:{valueOf() {return 4;}}};
console.log('lying-truth', truthScan(lyingBytes as any, 0), truthScan({0:0n,1:1n,2:{}} as any, 0));
console.log('lying-add', plusScan(lyingBytes as any, 0));
console.log('honest-oob-add', plusScan(new Uint8Array([3]), 0));
function replaceIndex(bytes: Uint8Array, i: number): any {
  for (; i < 2;) { i = bytes[3]; }
  return i;
}
console.log('assigned-oob-index', String(replaceIndex(new Uint8Array([1]), 0)), typeof replaceIndex(new Uint8Array([1]), 0));

function holdReadAcrossCollection(bytes: Uint8Array): string {
  const held: any = bytes[0];
  for (let i = 0; i < 8; i++) gc();
  return held.label;
}
console.log('boxed-read-gc', holdReadAcrossCollection({0:{label:'held'}} as any));

const nestedTable = new Uint8Array(256);
(nestedTable as any).undefined = 'hole';
function nestedAdd(bytes: Uint8Array, i: number): string {
  let out = '';
  for (; i < 2; i++) out += String(nestedTable[bytes[i]] + 1) + ',';
  return out;
}
console.log('nested-boxed-key', nestedAdd(new Uint8Array(0), 0));

function detachThroughArrayLength(bytes: Uint8Array, side: number[]): string {
  let out = '';
  for (let i = 0; i < 4; i++) {
    out += String(bytes[i]) + ',';
    if (i === 1) String(side.length);
  }
  return out;
}
const proxyBacking = new ArrayBuffer(4);
const proxyBytes = new Uint8Array(proxyBacking);
proxyBytes.set([3,5,7,11]);
const lengthProxy = new Proxy([], {get(target, key) {
  if (key === 'length') { proxyBacking.transfer(); return 4; }
  return Reflect.get(target, key);
}});
console.log('proxy-length-detach', detachThroughArrayLength(proxyBytes, lengthProxy));
