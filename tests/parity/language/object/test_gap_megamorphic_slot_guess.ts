// S3c: the inline shape-confirmed slot guess at a megamorphic read site.
//
// One read site (`readKind`) sees 60+ shapes, so it latches megamorphic and
// its reads take the inline guess: the site's slot guess is confirmed against
// the RECEIVER's own shape key list before any slot is loaded. Every receiver
// family below is built so that a guess which is NOT confirmed by the
// receiver's own shape would return a wrong value:
//   * `kind` at slot 2 in most shapes and at other slots in some (the guess
//     is right for some receivers, wrong for others);
//   * a DIFFERENT key sitting at the guessed slot;
//   * dictionary-mode receivers (many keys added one by one);
//   * receivers that deleted a key (tombstones / compaction);
//   * an accessor installed with defineProperty (descriptor shape);
//   * `kind` inherited from a prototype, and `kind` absent;
//   * class instances.
// The output is a per-family checksum and must equal node's.

const EXTRA = [];
for (let i = 0; i < 48; i++) EXTRA.push("x" + i);

function plain(i: number): any {
  const o: any = { pos: i, end: i + 1, kind: i % 7 };
  o[EXTRA[i % 48]] = i;
  return o;
}
function shifted(i: number): any {
  // `kind` at slot 4: the site's guess (2) names `flags` here.
  const o: any = { pos: i, end: i + 1, flags: 1000 + i, parent: null, kind: 50 + (i % 5) };
  o[EXTRA[(i + 7) % 48]] = i;
  return o;
}
function dictionary(i: number): any {
  const o: any = { pos: i, end: i + 1, kind: 200 + (i % 3) };
  for (let k = 0; k < 200; k++) o["d" + i + "_" + k] = k;
  return o;
}
function deleted(i: number): any {
  const o: any = { pos: i, end: i + 1, kind: 300 + (i % 4), gone: 1, also: 2 };
  o[EXTRA[(i + 3) % 48]] = i;
  delete o.also;
  delete o.end;
  return o;
}
function accessor(i: number): any {
  const o: any = { pos: i, end: i + 1, kind: -1 };
  Object.defineProperty(o, "kind", { get() { return 400 + (i % 6); }, enumerable: true });
  return o;
}
const proto = { kind: 500 };
function inherited(i: number): any {
  const o: any = Object.create(proto);
  o.pos = i;
  o.end = i + 1;
  o.flags = i;
  return o;
}
function absent(i: number): any {
  const o: any = { pos: i, end: i + 1, flags: 600 + i };
  o[EXTRA[(i + 11) % 48]] = i;
  return o;
}
class Node3 {
  pos: number; end: number; kind: number;
  constructor(i: number) { this.pos = i; this.end = i + 1; this.kind = 700 + (i % 9); }
}

function readKind(node: any): any {
  return node.kind;
}

const families: [string, (i: number) => any][] = [
  ["plain", plain], ["shifted", shifted], ["dictionary", dictionary], ["deleted", deleted],
  ["accessor", accessor], ["inherited", inherited], ["absent", absent],
  ["class", (i: number) => new Node3(i)],
];
const pool: [number, any][] = [];
for (let f = 0; f < families.length; f++) {
  for (let i = 0; i < 64; i++) pool.push([f, families[f][1](i)]);
}
// Interleave so the site sees every family while latched.
const order: number[] = [];
for (let i = 0; i < pool.length; i++) order.push((i * 37) % pool.length);

const sums: number[] = families.map(() => 0);
let undef = 0;
for (let round = 0; round < 50; round++) {
  for (const j of order) {
    const [f, o] = pool[j];
    const v = readKind(o);
    if (v === undefined) undef++;
    else sums[f] += v;
  }
  // Mutate some receivers between rounds: a moved key, a re-added key.
  if (round === 20) {
    for (const [f, o] of pool) {
      if (f === 0 && o.pos % 5 === 0) { delete o.kind; o.kind = 900; }
    }
  }
}
for (let f = 0; f < families.length; f++) console.log(families[f][0] + " " + sums[f]);
console.log("undefined " + undef);
