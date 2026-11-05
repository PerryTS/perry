// parity-node-argv: --expose-gc
// #13792: an array that grows (push, unshift, splice, length, index store past
// capacity) keeps its identity for every identity-keyed table. Growth moves
// the array and leaves a forwarding stub, so the binding that grew holds the
// new head while tables and aliases taken before hold the old one.
declare function gc(): void;

function grow(a: any[], how: string): void {
  if (how === "push") {
    for (let i = 0; i < 300; i++) a.push(i);
  } else if (how === "unshift") {
    for (let i = 0; i < 300; i++) a.unshift(i);
  } else if (how === "splice") {
    a.splice(0, 0, ...new Array(600).fill(1));
  } else if (how === "length") {
    a.length = 5000;
    a[4999] = 1;
  } else {
    for (let i = 0; i < 600; i++) a[a.length] = i;
  }
}

// The binding that grows is the probe: inline growth, so `a` is refreshed.
function inline(how: string, n: number, collect: boolean): string {
  const a: any[] = [];
  for (let i = 0; i < n; i++) a.push(i);
  const alias = a;
  const wm = new WeakMap<object, number>();
  const ws = new WeakSet<object>();
  const small = new Map<any, number>();
  const large = new Map<any, number>();
  const set = new Set<any>();
  const wr = new WeakRef(a);
  const fr = new FinalizationRegistry<number>(() => {});
  for (let i = 0; i < 64; i++) {
    large.set({ i }, i);
    set.add({ i });
  }
  wm.set(a, 7);
  ws.add(a);
  small.set(a, 8);
  large.set(a, 9);
  set.add(a);
  fr.register({}, 1, a);
  if (how === "push") {
    for (let i = 0; i < 300; i++) a.push(i);
  } else if (how === "unshift") {
    for (let i = 0; i < 300; i++) a.unshift(i);
  } else if (how === "splice") {
    a.splice(0, 0, ...new Array(600).fill(1));
  } else if (how === "length") {
    a.length = 5000;
    a[4999] = 1;
  } else {
    for (let i = 0; i < 600; i++) a[a.length] = i;
  }
  if (collect) gc();
  const before = large.size;
  large.set(a, 10);
  small.set(a, 11);
  const out = [
    wm.get(a), wm.has(a), wm.get(alias), ws.has(a), ws.has(alias),
    small.get(a), small.get(alias), small.size,
    large.get(a), large.get(alias), large.size === before,
    set.has(a), set.has(alias),
    wr.deref() === a, a === alias, Object.is(a, alias), Object.is(alias, a),
  ];
  // Delete through the other name, then the entry is gone under both.
  wm.delete(alias);
  ws.delete(a);
  large.delete(alias);
  small.delete(a);
  out.push(wm.has(a), ws.has(alias), large.has(a), large.has(alias), small.has(alias));
  out.push(fr.unregister(a), fr.unregister(alias));
  return out.join(",");
}

// The probe is the alias; growth happened in a callee.
function callee(how: string, n: number): string {
  const a: any[] = [];
  for (let i = 0; i < n; i++) a.push(i);
  const wm = new WeakMap<object, number>();
  const m = new Map<any, number>();
  wm.set(a, 1);
  m.set(a, 2);
  grow(a, how);
  return [wm.get(a), m.get(a), Object.is(a, a)].join(",");
}

for (const how of ["push", "unshift", "splice", "length", "index"]) {
  for (const n of [0, 3, 1000]) {
    console.log(how, n, inline(how, n, false));
    console.log(how, n, "gc", inline(how, n, true));
    console.log(how, n, "callee", callee(how, n));
  }
}

// Many array keys: the token index keeps lookups exact after growth.
const keys: any[][] = [];
const wm = new WeakMap<object, number>();
const m = new Map<any, number>();
for (let i = 0; i < 200; i++) {
  const k: any[] = [i];
  keys.push(k);
  wm.set(k, i);
  m.set(k, i);
}
let hits = 0;
for (let i = 0; i < keys.length; i++) {
  const k = keys[i];
  for (let j = 0; j < 40; j++) k.push(j);
  if (wm.get(k) === i && m.get(k) === i) hits++;
}
console.log("many", hits, m.size, wm.has([0]), m.has([0]));
