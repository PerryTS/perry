// `arr.push(f(..))` where whole-program facts prove the call's result a
// Number takes the inline push (PERRY_FACTS_DIRECT_NUMBER_CALLS). Exact
// against node with the rule on, off and in check mode.
// A loop bound the compiler cannot fold keeps each loop as written.
const RUN = Math.random() < 2 ? 1 : 0;

function sum(a: number[]): string {
  let s = 0;
  for (let i = 0; i < a.length; i++) s = (s * 31 + (a[i] as number)) % 1000003;
  return a.length + ":" + s;
}

// 1. The shape: a move list filled with the result of a direct call, with
// and without default parameters, from a method and through a helper method.
function encode(from: number, to: number, promotion = 0, flags = 0): number {
  return from | (to << 7) | (promotion << 14) | flags;
}
function enc2(from: number, to: number): number {
  return from | (to << 7);
}
function half(x: number): number {
  let r = 0;
  for (let k = 0; k < 2 * RUN; k++) r += x / 4;
  return r;
}
class Gen {
  board: number[] = new Array<number>(128).fill(0);
  at(square: number): number {
    return this.board[square] ?? 0;
  }
  add(moves: number[], from: number, to: number): void {
    moves.push(encode(from, to, 0, 4));
  }
  gen1(): number[] {
    const moves: number[] = [];
    for (let from = 0; from < 128 * RUN; from++) {
      if (this.at(from) === 0) moves.push(encode(from, from + 1));
    }
    return moves;
  }
  gen2(): number[] {
    const moves: number[] = [];
    for (let from = 0; from < 128 * RUN; from++) {
      if (this.at(from) === 0) moves.push(enc2(from, from + 1));
    }
    return moves;
  }
  gen3(): number[] {
    const moves: number[] = [];
    for (let from = 0; from < 128 * RUN; from++) this.add(moves, from, from + 1);
    return moves;
  }
  gen4(): number[] {
    const moves: number[] = [];
    for (let from = 0; from < 300 * RUN; from++) moves.push(half(from));
    return moves;
  }
}
const g = new Gen();
console.log(sum(g.gen1()), sum(g.gen2()), sum(g.gen3()), sum(g.gen4()));

// 2. Growth from empty past several capacities, the returned length, and
// the value of the push expression.
const grown: number[] = [];
let lens = 0;
for (let i = 0; i < 5000 * RUN; i++) lens += grown.push(enc2(i & 127, i >> 7));
console.log(sum(grown), lens, grown[4999], grown[5000]);

// 3. Fractions, -0, NaN and infinities stay exactly what the call returned.
function special(k: number): number {
  let r = 0;
  for (let i = 0; i < 1 * RUN; i++) r += 0;
  if (k === 0) return -0;
  if (k === 1) return NaN;
  if (k === 2) return Infinity;
  if (k === 3) return -Infinity;
  if (k === 4) return 2 ** 53;
  if (k === 5) return -(2 ** 31);
  return k + 0.5 + r;
}
const sp: number[] = [];
for (let k = 0; k < 8 * RUN; k++) sp.push(special(k));
console.log(sp.map((v) => (Object.is(v, -0) ? "-0" : String(v))).join(","));

// 4. A NaN with a payload read from a typed array is a NaN in the array.
const cells = new Float64Array(2);
const words = new Uint32Array(cells.buffer);
words[0] = 1;
words[1] = 0x7ffd0000;
words[2] = 7;
words[3] = 0xfffc0000;
function payload(i: number): number {
  let r = 0;
  for (let k = 0; k < 1 * RUN; k++) r += k;
  return cells[i + r] as number;
}
const nans: number[] = [];
for (let i = 0; i < 2 * RUN; i++) nans.push(payload(i));
nans.push(1.5);
console.log(nans.length, typeof nans[0], String(nans[0]), String(nans[1]), nans[2]);

// 5. A call through a function value, a closure and a conditional callee.
const viaValue = enc2;
const scale = (k: number) => (x: number): number => {
  let r = 0;
  for (let i = 0; i < k * RUN; i++) r += x;
  return r;
};
const triple = scale(3);
const picked = RUN ? enc2 : encode;
const fv: number[] = [];
for (let i = 0; i < 40 * RUN; i++) {
  fv.push(viaValue(i, 2));
  fv.push(triple(i));
  fv.push(picked(i, 3));
  fv.push(scale(2)(i));
}
console.log(sum(fv));

// Negative controls: each keeps the guarded push.

// 6. A callee declared `number` that returns something else.
function liar(k: number): number {
  let r = 0;
  for (let i = 0; i < 1 * RUN; i++) r += i;
  if (k === 1) return "str" as any;
  if (k === 2) return undefined as any;
  if (k === 3) return null as any;
  if (k === 4) return true as any;
  if (k === 5) return { v: 1 } as any;
  if (k === 6) return 10n as any;
  return k + r;
}
const mixed: number[] = [];
for (let k = 0; k < 8 * RUN; k++) mixed.push(liar(k));
console.log(mixed.map((v) => typeof v + ":" + String(v)).join(","));

// 7. A call that may complete without a return, and a read past the end.
function maybe(k: number): number {
  let r = 0;
  for (let i = 0; i < 1 * RUN; i++) r += i;
  if (k % 2 === 0) return k + r;
  return undefined as unknown as number;
}
const src = [1, 2, 3];
function past(k: number): number {
  let r = 0;
  for (let i = 0; i < 1 * RUN; i++) r += i;
  return src[k + r] as number;
}
const und: number[] = [];
for (let k = 0; k < 5 * RUN; k++) {
  und.push(maybe(k));
  und.push(past(k));
}
console.log(und.map((v) => String(v)).join(","));

// 8. Either of two callees, one of which returns a string.
function word(k: number): number {
  let r = "w";
  for (let i = 0; i < 1 * RUN; i++) r += k;
  return r as any;
}
const either: number[] = [];
for (let k = 0; k < 4 * RUN; k++) either.push((k % 2 ? word : enc2)(k, 1));
console.log(either.map((v) => typeof v).join(","));

// 9. An array a generic reader sees, and one that holds other kinds first.
function show<T>(items: T[]): string {
  let out = "";
  for (const item of items) out += typeof item + String(item) + ";";
  return out;
}
const generic: any[] = [];
generic.push("first");
for (let k = 0; k < 3 * RUN; k++) generic.push(enc2(k, 1));
generic.push({ toString: () => "obj" });
console.log(show(generic));
const numbersOnly: number[] = [];
for (let k = 0; k < 3 * RUN; k++) numbersOnly.push(enc2(k, 1));
console.log(show(numbersOnly));

// 10. Holey receivers: a preset length, a length set past the end, a delete.
const holey: number[] = new Array<number>(3);
holey.push(enc2(1, 1));
const stretched: number[] = [];
stretched.push(enc2(2, 1));
stretched.length = 4 * RUN;
stretched.push(enc2(3, 1));
const deleted: number[] = [1, 2, 3];
delete deleted[1];
deleted.push(enc2(4, 1));
console.log(holey.length, 0 in holey, holey[3], stretched.length, 2 in stretched, stretched[4]);
console.log(deleted.length, 1 in deleted, deleted[3], JSON.stringify(deleted));

// 11. Frozen, sealed and non-extensible receivers, and a read-only length.
function tryPush(a: number[], label: string): void {
  try {
    a.push(enc2(5, 1));
    console.log(label, "pushed", a.length);
  } catch (e) {
    console.log(label, (e as Error).constructor.name, a.length);
  }
}
tryPush(Object.freeze([1, 2]) as number[], "frozen");
tryPush(Object.seal([1, 2]) as number[], "sealed");
tryPush(Object.preventExtensions([1, 2]) as number[], "nonext");
const fixedLength: number[] = [1, 2];
Object.defineProperty(fixedLength, "length", { writable: false });
tryPush(fixedLength, "readonly-length");

// 12. An own `push`, an accessor element and a subclass.
const own: number[] = [];
(own as any).push = function (v: number): number {
  return -v;
};
console.log(own.push(enc2(6, 1)), own.length);
class Stack extends Array<number> {
  push(...items: number[]): number {
    return super.push(...items.map((v) => v + 1));
  }
}
const st = new Stack();
for (let k = 0; k < 3 * RUN; k++) st.push(enc2(k, 0));
console.log(st.length, st[0], st[2]);

// 13. The argument's call rebinds, grows or empties the receiver.
let rebound: number[] = [];
function rebind(k: number): number {
  rebound = [100];
  return k;
}
const before = rebound;
before.push(1);
rebound.push(rebind(7));
console.log(before.length, rebound.length, before[1], rebound[1]);
const self: number[] = [];
function reenter(k: number): number {
  if (k < 20 * RUN) self.push(k);
  if (k === 30) self.length = 0;
  return k * 2;
}
for (let k = 0; k < 40 * RUN; k++) self.push(reenter(k));
console.log(sum(self));
