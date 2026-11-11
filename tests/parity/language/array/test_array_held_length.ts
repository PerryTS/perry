// Held lengths (wpfacts `held_lengths`): a loop over a local Array holds
// `a.length` across the calls in its body when the Array lives in frames
// only, the function hands it to no call and has no length event on it.
// The P cases hold the length; every N case is a negative control where the
// length does change (or may) during the loop, and the loop must see it.

let sink = 0;
function touch(v: number): number {
  sink += v;
  return v & 3;
}

function gen(n: number): number[] {
  const out: number[] = [];
  for (let i = 0; i < n; i++) out.push(i * 3 + 1);
  return out;
}

// The same births for the negative controls: an Array token is its birth
// site, so a control that puts `gen`'s Array into a field would end the
// fact for every P case too.
function mk(n: number): number[] {
  const out: number[] = [];
  for (let i = 0; i < n; i++) out.push(i * 3 + 1);
  return out;
}

// Pushes through a parameter.
function fill(out: number[], n: number): void {
  for (let i = 0; i < n; i++) out.push(out.length * 2);
}

function gen2(n: number): number[] {
  const out: number[] = [];
  fill(out, n);
  return out;
}

// P1: for-of over a fresh call result, a call in the body.
function p1(n: number): number {
  let s = 0;
  for (const v of gen(n)) s += touch(v) + v;
  return s;
}

// P2: recursion. The function that bore the walked Array runs again below
// the loop and grows its own Array of the same birth site.
function p2(depth: number): number {
  let s = 0;
  for (const v of gen2(4)) {
    s += v + 1;
    if (depth > 0) s += p2(depth - 1);
  }
  return s;
}

// P3: a counted loop over a local; the body allocates, so a collection may
// move the Array while its length is held.
function p3(n: number): number {
  const xs = gen(n);
  let s = 0;
  let junk: number[][] = [];
  for (let i = 0; i < xs.length; i++) {
    junk.push([i, i + 1, i + 2]);
    if (junk.length > 50) junk = [];
    s += (xs[i] ?? -1000) + touch(i);
  }
  return s + junk.length;
}

// P4: a parameter is walked; a different Array is handed down and grows.
function p4(xs: number[]): number {
  let s = 0;
  const other: number[] = [5, 6, 7];
  for (const v of xs) {
    s += v + other.length;
    fill(other, 1);
  }
  return s + other.length;
}

// P5: a young Array walked while the callee allocates enough to collect.
function churn(k: number): number {
  const tmp: number[][] = [];
  for (let i = 0; i < 40; i++) tmp.push([k, i, k + i]);
  return tmp.length + ((tmp[k % 40] ?? [])[2] ?? 0);
}
function p5(rounds: number): number {
  let s = 0;
  for (let r = 0; r < rounds; r++) {
    for (const v of gen(24)) s = (s + v + churn(v)) % 1000003;
  }
  return s;
}

// N1: the callee pushes onto the walked Array, handed down as an argument.
function n1(): string {
  const xs = mk(3);
  const seen: number[] = [];
  for (const v of xs) {
    seen.push(v);
    if (xs.length < 7) fill(xs, 1);
  }
  return seen.join(",") + "|" + xs.length;
}

// N2: the callee pops.
function popOne(a: number[]): void {
  a.pop();
}
function n2(): string {
  const xs = mk(6);
  const seen: number[] = [];
  for (const v of xs) {
    seen.push(v);
    popOne(xs);
  }
  return seen.join(",") + "|" + xs.length;
}

// N3: the callee sets `length`.
function cut(a: number[], n: number): void {
  a.length = n;
}
function n3(): string {
  const xs = mk(8);
  const seen: number[] = [];
  for (let i = 0; i < xs.length; i++) {
    seen.push(xs[i] ?? -1);
    if (i === 2) cut(xs, 4);
  }
  return seen.join(",") + "|" + xs.length;
}

// N4: the Array is a field's; a method grows it through the field.
// N5: a method assigns the field; the loop keeps walking the old Array.
class Holder {
  items: number[] = mk(3);
  grow(): void {
    if (this.items.length < 8) this.items.push(100 + this.items.length);
  }
  replace(): void {
    this.items = [7, 7];
  }
  walk(): string {
    const xs = this.items;
    const seen: number[] = [];
    for (const v of xs) {
      seen.push(v);
      this.grow();
    }
    return seen.join(",") + "|" + xs.length;
  }
  walkReplaced(): string {
    const xs = this.items;
    const seen: number[] = [];
    for (const v of xs) {
      seen.push(v);
      this.replace();
      this.grow();
    }
    return seen.join(",") + "|" + xs.length + "|" + this.items.length;
  }
}

// N6: a closure captures the Array and pushes; it is called in the loop.
function n6(): string {
  const xs = mk(3);
  const grow = () => {
    if (xs.length < 6) xs.push(9);
  };
  const seen: number[] = [];
  for (const v of xs) {
    seen.push(v);
    grow();
  }
  return seen.join(",") + "|" + xs.length;
}

// N7: a module binding holds the Array; the callee is picked at run time.
let shared: number[] = [];
const ops: Array<() => void> = [
  () => {
    shared.push(shared.length);
  },
  () => {
    shared.pop();
  },
  () => {},
];
function n7(k: number): string {
  shared = mk(4);
  const xs = shared;
  const seen: number[] = [];
  let i = 0;
  for (const v of xs) {
    seen.push(v);
    const op = ops[(i * 7 + k) % 3];
    if (op !== undefined) op();
    i++;
    if (i > 12) break;
  }
  return seen.join(",") + "|" + xs.length;
}

// N8: the loop's own body pushes.
function n8(): string {
  const xs = mk(3);
  const seen: number[] = [];
  for (const v of xs) {
    seen.push(v);
    if (xs.length < 6) xs.push(v + 100);
  }
  return seen.join(",") + "|" + xs.length;
}

// N9: an alias goes into another Array's element; the callee reads it back.
function growBox(box: number[][]): void {
  const a = box[0];
  if (a !== undefined && a.length < 6) a.push(7);
}
function n9(): string {
  const xs = mk(3);
  const box = [xs];
  const seen: number[] = [];
  for (const v of xs) {
    seen.push(v);
    growBox(box);
  }
  return seen.join(",") + "|" + xs.length;
}

// N10: a generator's frame holds the Array and pushes when it is resumed.
function* feeder(a: number[]): Generator<number> {
  while (a.length < 6) {
    a.push(a.length * 2);
    yield a.length;
  }
}
function n10(): string {
  const xs = mk(2);
  const it = feeder(xs);
  const seen: number[] = [];
  for (const v of xs) {
    seen.push(v);
    it.next();
  }
  return seen.join(",") + "|" + xs.length;
}

// N11: the callee shifts (the elements move) and unshifts.
function rotate(a: number[]): void {
  const first = a.shift();
  if (first !== undefined && a.length < 5) a.unshift(first + 1, first + 2);
}
function n11(): string {
  const xs = mk(4);
  const seen: number[] = [];
  let guard = 0;
  for (const v of xs) {
    seen.push(v);
    rotate(xs);
    if (++guard > 9) break;
  }
  return seen.join(",") + "|" + xs.join(",");
}

console.log("p1", p1(10), p1(0), sink);
console.log("p2", p2(3));
console.log("p3", p3(200), p3(1));
console.log("p4", p4(gen(5)), p4([]));
console.log("p5", p5(1500));
console.log("n1", n1());
console.log("n2", n2());
console.log("n3", n3());
const holder = new Holder();
console.log("n4", holder.walk(), holder.items.length);
console.log("n5", holder.walkReplaced());
console.log("n6", n6());
console.log("n7", n7(0), n7(1), n7(2));
console.log("n8", n8());
console.log("n9", n9());
console.log("n10", n10());
console.log("n11", n11());
