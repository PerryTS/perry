// An index store whose key a length guard bounds (`k <= i < a.length`)
// overwrites an element or appends: it leaves no hole. The whole-program
// facts keep such an Array's for-of element a Number (no `undefined`), so a
// value read from a sorted copy stays an int32 through typed calls. Every
// case prints what node prints; the negative controls are the stores (and
// the shrinks) that do leave a hole or may.
const many = process.argv.length > 100;

function enc(a: number, b: number): number {
  return a | (b << 7);
}
function gen(n: number, seed: number): number[] {
  const out: number[] = [];
  for (let i = 0; i < n; i++) out.push(enc((i * 37 + seed) & 127, (i * 11 + seed * 3) & 127));
  return out;
}
// The consumer: a typed call that takes the element as a Number.
function use(m: number): number {
  return ((m & 127) * 3 + ((m >> 7) & 127)) | 0;
}
function total(xs: readonly number[]): string {
  let s = 0;
  let holes = 0;
  for (const x of xs) {
    if (x === undefined) holes++;
    else s = (s + use(x)) | 0;
  }
  return s + "/" + holes + "/" + xs.length;
}

// (a) The insertion sort of a copy, with a parallel key array: every store
// into `result` is at `j + 1 <= i < result.length`.
function order(moves: readonly number[], preferred: number): number[] {
  const result = moves.slice();
  const scores: number[] = [];
  for (const move of result) scores.push(move === preferred ? 1000000 : (move * 7919) % 1009);
  for (let i = 1; i < result.length; i++) {
    const move = result[i] ?? 0;
    const score = scores[i] ?? 0;
    let j = i - 1;
    while (j >= 0 && (scores[j] ?? 0) < score) {
      result[j + 1] = result[j] ?? 0;
      scores[j + 1] = scores[j] ?? 0;
      j--;
    }
    result[j + 1] = move;
    scores[j + 1] = score;
  }
  return result;
}
let acc = 0;
for (let r = 0; r < 200; r++) {
  const ms = gen(5 + (r % 23), r);
  const sorted = order(ms, ms[r % ms.length] ?? 0);
  for (const m of sorted) acc = (acc + use(m)) | 0;
  if (r < 3) console.log("sort", sorted.join(","), total(ms));
}
console.log("sort many", acc);
console.log("sort empty", order([], 0).length, order(gen(1, 4), 0).join(","));

// (b) Stores at the counter, below it and from a counter that only falls.
function stores(n: number): string {
  const xs = gen(n, 3);
  for (let i = 0; i < xs.length; i++) xs[i] = (xs[i] ?? 0) ^ 5;
  for (let i = 2; i < xs.length; i++) xs[i - 2] = (xs[i] ?? 0) + 1;
  for (let i = xs.length - 1; i >= 0; i--) {
    if (i < xs.length) xs[i] = (xs[i] ?? 0) | 1;
  }
  for (let i = 1; i < xs.length; i++) {
    for (let j = i - 1; j >= 0; j -= 2) xs[j] = ((xs[j] ?? 0) + i) & 16383;
  }
  let k = 0;
  while (k < xs.length) {
    xs[k] = (xs[k] ?? 0) + 2;
    k += 3;
  }
  return total(xs);
}
console.log("stores", stores(0), stores(1), stores(17));

// (c) An append at the length through a guard that allows it is not what
// the proof covers (`k <= i < length` only), but it leaves no hole either.
function appends(n: number): string {
  const xs = gen(n, 9);
  for (let i = 0; i < 6; i++) xs[xs.length] = i;
  return total(xs);
}
console.log("appends", appends(0), appends(4));

// ---- negative controls ----

// (d) A store past the end makes holes.
function past(n: number): string {
  const xs = gen(n, 1);
  for (let i = 0; i < xs.length && i < 4; i++) xs[i + 3] = 7;
  xs[xs.length + 2] = 9;
  return total(xs);
}
console.log("past", past(2), past(6));

// (e) A length store grows the Array with holes; one shrinks it under a
// loop whose guard ran before.
function lengths(n: number): string {
  const xs = gen(n, 2);
  for (let i = 0; i < xs.length; i++) xs[i] = 1;
  xs.length = n + 3;
  const ys = gen(n, 2);
  for (let i = 0; i < ys.length; i++) {
    if (i === 2) ys.length = 1;
    ys[i] = 5;
  }
  return total(xs) + " " + total(ys);
}
console.log("lengths", lengths(1), lengths(8));

// (f) A delete leaves a hole.
function deleted(n: number): string {
  const xs = gen(n, 5);
  for (let i = 0; i < xs.length; i++) xs[i] = 2;
  delete xs[1];
  return total(xs);
}
console.log("deleted", deleted(4));

// (g) A value that is no Number is pushed: the element is what it is.
function mixed(n: number): string {
  const xs: any[] = gen(n, 6);
  for (let i = 0; i < xs.length; i++) xs[i] = xs[i] + 1;
  xs.push(many ? 0 : "seven");
  let s = "";
  for (const x of xs) s += typeof x === "number" ? "n" : String(x);
  return s;
}
console.log("mixed", mixed(3));

// (h) A pop inside the loop body, below the index the store uses.
function popped(n: number): string {
  const xs = gen(n, 7);
  for (let i = 0; i < xs.length; i++) {
    if (i % 3 === 1) {
      xs.pop();
      xs.pop();
    }
    xs[i] = 100 + i;
  }
  return total(xs);
}
console.log("popped", popped(2), popped(5), popped(9));

// (i) The Array is shortened through another name, in a callee.
function shorten(a: number[], by: number): void {
  for (let k = 0; k < by; k++) a.pop();
}
function aliased(n: number): string {
  const xs = gen(n, 8);
  const other = xs;
  for (let i = 0; i < xs.length; i++) {
    if (i === 1) shorten(other, 3);
    xs[i] = 200 + i;
  }
  return total(xs);
}
console.log("aliased", aliased(2), aliased(4), aliased(10));

// (j) Keys the guard does not bound: a key past the counter, a counter the
// body moves first, a local that also rises, a guard on another Array, a
// key from a call, and a counter that is an object.
function unproven(n: number): string {
  const a = gen(n, 10);
  for (let i = 0; i < a.length && i < 4; i++) a[i + 2] = 1;
  const b = gen(n, 10);
  for (let i = 0; i < b.length && i < 7; i++) {
    i += 2;
    b[i] = 2;
  }
  const c = gen(n, 10);
  for (let i = 1; i < c.length && i < 5; i++) {
    let j = i - 1;
    j += 3;
    c[j] = 3;
  }
  const d = gen(n, 10);
  const longer = gen(n + 4, 10);
  for (let i = 0; i < longer.length; i++) d[i] = 4;
  const e = gen(n, 10);
  for (let i = 0; i < e.length; i++) e[use(i) % 9] = 5;
  return [total(a), total(b), total(c), total(d), total(e)].join(" ");
}
console.log("unproven", unproven(1), unproven(3));
let calls = 0;
const sly: any = { valueOf: () => (calls++ === 0 ? 0 : 5) };
function coerced(): string {
  const xs = gen(2, 11);
  const i = sly;
  if (i < xs.length) xs[i - 0] = 6;
  return total(xs);
}
console.log("coerced", coerced());

// (k) A key that is no index: a named property, no hole.
function named(n: number): string {
  const xs = gen(n, 12);
  for (let i = 0; i < xs.length; i++) {
    let j = i - 1;
    xs[j] = 8;
    xs[i - 0.5] = 9;
  }
  return total(xs) + " " + Object.keys(xs).length;
}
console.log("named", named(0), named(3));

// (l) The sorted copy's values through a class, as a search uses them.
class Search {
  private best = 0;
  private table = new Map<string, { key: string; move: number }>();
  play(move: number, depth: number): number {
    return use(move) + depth;
  }
  run(depth: number, seed: number): number {
    let n = 0;
    const key = "k" + (seed & 7);
    const hit = this.table.get(key);
    const preferred = hit === undefined ? 0 : hit.move;
    for (const move of order(gen(6 + (seed & 3), seed), preferred)) {
      n = (n + this.play(move, depth)) | 0;
      if ((move & 3) === 1) this.best = move;
      if (depth > 0) n = (n + this.run(depth - 1, (seed * 5 + 1) & 63)) | 0;
    }
    this.table.set(key, { key, move: this.best });
    return n;
  }
}
console.log("search", new Search().run(2, 3));
