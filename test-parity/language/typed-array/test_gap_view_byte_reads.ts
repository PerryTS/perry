import { Buffer } from 'node:buffer';
function scan(bytes: Uint8Array): number {
  let sum = 0;
  for (let i = 0; i < bytes.length; i++) sum += bytes[i]!;
  return sum;
}
function get(bytes: Uint8Array, key: any, rounds: number = 1): any {
  let result: any = undefined;
  for (let r = 0; r < rounds; r++) result = bytes[key];
  return result;
}
const owner = new Uint8Array(32);
for (let i = 0; i < owner.length; i++) owner[i] = i + 1;
const view = new Uint8Array(owner.buffer, 5, 12);
const sub = owner.subarray(5, 17).subarray(2, 10);
const buffer = Buffer.from(owner.buffer, 5, 12);
console.log('sums', scan(owner), scan(view), scan(sub), scan(buffer));
const words = new Uint16Array(owner.buffer);
words[4] = 0x0707;
console.log('alias', get(sub, 1), get(sub, 2), scan(sub));
for (const key of [0, -0, 7, 8, -1, 1.5, 4294967296, NaN, Infinity, '0', '-0', '01', '7', '8']) {
  console.log('key', String(key), get(sub, key));
}
// Exercise an untruthful annotation through the same read sites.
console.log('ordinary', get([21, 22] as any, 1), get({0: 23} as any, 0));
const rab = new ArrayBuffer(16, {maxByteLength: 32});
const fixed = new Uint8Array(rab, 4, 8);
const tracking = new Uint8Array(rab, 4);
fixed.fill(9);
console.log('rab', scan(fixed), scan(tracking), fixed.length, tracking.length);
rab.resize(6);
console.log('shrunk', scan(fixed), scan(tracking), fixed.length, tracking.length, get(fixed, 0), get(tracking, 2));
rab.resize(24);
console.log('grown', scan(fixed), scan(tracking), fixed.length, tracking.length, get(fixed, 2));
rab.transfer();
console.log('detached', scan(fixed), scan(tracking), fixed.length, tracking.length, get(fixed, 0));
const shared = new SharedArrayBuffer(32);
const left = new Uint8Array(shared, 3, 12);
const right = new Uint8Array(shared, 5, 8);
left.fill(4);
Atomics.store(right, 2, 99);
console.log('shared', scan(left), scan(right), get(left, 4));
Object.defineProperty(view, 'length', {value: 3});
console.log('own length', view.length, scan(view));
// An index coercion can detach/resize before the access happens.
const detachBacking = new ArrayBuffer(8);
const detachView = new Uint8Array(detachBacking); detachView.fill(6);
const detachKey = {toString() {detachBacking.transfer(); return '0';}};
console.log('coercion', get(detachView, detachKey), detachView.length);
// Keep views reachable through moving ordinary objects while loops allocate.
function churn(): number {
  const backing = new Uint8Array(64); backing.fill(11);
  const holder = {bytes: backing.subarray(5, 37), last: {text: ''}};
  let sum = 0;
  for (let i = 0; i < 200; i++) {
    holder.last = {text: 'allocation-' + i};
    const ephemeral = {items: [i, i + 1], text: holder.last.text + '!'};
    sum += get(holder.bytes, i & 31) + ephemeral.items[0];
  }
  return sum;
}
console.log('gc holders', churn());

// Exercise detach and resize inside the lifetime of one entry proof.
function mutateDuringReads(bytes: Uint8Array, backing: ArrayBuffer, rounds: number): void {
  for (let r = 0; r < rounds; r++) {
  console.log("live before", bytes[0], bytes.length);
  backing.resize(2);
  console.log("live shrink", bytes[0], bytes.length);
  backing.resize(16);
  console.log("live grow", bytes[0], bytes[7], bytes.length);
  backing.transfer();
  console.log("live detach", bytes[0], bytes.length);
  }
}
const liveBacking = new ArrayBuffer(16, {maxByteLength: 32});
const liveView = new Uint8Array(liveBacking, 4, 8); liveView.fill(13);
mutateDuringReads(liveView, liveBacking, 1);

function detachOwnerDuringReads(bytes: Uint8Array, rounds: number): void {
  for (let r = 0; r < rounds; r++) {
  console.log('owner before', bytes[0], bytes.length);
  bytes.buffer.transfer();
  console.log('owner detach', bytes[0], bytes.length);
  }
}
const liveOwner = new Uint8Array(8); liveOwner.fill(15);
detachOwnerDuringReads(liveOwner, 1);
