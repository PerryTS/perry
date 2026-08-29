// #9019: an own/patched `.next` on a builtin collection iterator must drive
// iteration (for-of, spread, manual calls), and a named property write on an
// iterator object must not corrupt its internal state. Pre-fix, the first
// named write landed at field index 0 — the backing-collection pointer — so
// `it.foo = 1` made iteration report done immediately and `it.next = fn`
// crashed the next builtin advance with SIGSEGV.

// A: the issue reproducer — for-of over a Set iterator with a patched next.
{
  const s = new Set<number>([10, 20, 30, 40]);
  const it: any = s.values();
  let calls = 0;
  const orig = it.next.bind(it);
  it.next = function () { calls++; return orig(); };
  const got: number[] = [];
  for (const v of it) got.push(v as number);
  console.log("A", calls > 0, got.join(","));
}

// B: manual .next() drive of the same patch shape.
{
  const s = new Set<number>([1, 2, 3]);
  const it: any = s.values();
  let calls = 0;
  const orig = it.next.bind(it);
  it.next = function () { calls++; return orig(); };
  const got: number[] = [];
  let r = it.next();
  while (!r.done) { got.push(r.value as number); r = it.next(); }
  console.log("B", calls, got.join(","));
}

// C-F: a plain named write must not disturb iteration, per family.
{
  const m = new Map<string, number>([["a", 1], ["b", 2]]);
  const mi: any = m.entries();
  mi.foo = 123;
  console.log("C", JSON.stringify(mi.next()), mi.foo);
  const s = new Set<number>([7]);
  const si: any = s.values();
  si.foo = 9;
  console.log("D", JSON.stringify(si.next()));
  const ai: any = [5, 6].values();
  ai.foo = 9;
  console.log("E", JSON.stringify(ai.next()));
  const ti: any = "xy"[Symbol.iterator]();
  ti.foo = 9;
  console.log("F", JSON.stringify(ti.next()));
}

// G: patched next on a Map iterator, driven by for-of, rewriting values.
{
  const m = new Map<string, number>([["a", 1], ["b", 2]]);
  const it: any = m.entries();
  const orig = it.next.bind(it);
  it.next = function () {
    const r = orig();
    if (!r.done) r.value = [r.value[0], (r.value[1] as number) * 10];
    return r;
  };
  const got: string[] = [];
  for (const [k, v] of it) got.push(k + "=" + v);
  console.log("G", got.join(","));
}

// H: spread honors the patch too.
{
  const s = new Set<number>([1, 2]);
  const it: any = s.values();
  const orig = it.next.bind(it);
  let calls = 0;
  it.next = function () { calls++; return orig(); };
  console.log("H", [...it].join(","), calls > 0);
}

// I: a non-callable own next throws TypeError when for-of drives it.
{
  const s = new Set<number>([1]);
  const it: any = s.values();
  it.next = 42;
  try {
    for (const v of it) console.log("I-unexpected", v);
    console.log("I", "no-throw");
  } catch (e: any) {
    console.log("I", e instanceof TypeError);
  }
}

// J: the added property is an ordinary own enumerable.
{
  const s = new Set<number>([1]);
  const it: any = s.values();
  it.foo = 5;
  console.log("J", JSON.stringify(Object.keys(it)), JSON.stringify(it));
}

// K: deleting the patch restores the builtin advance.
{
  const s = new Set<number>([1, 2]);
  const it: any = s.values();
  it.next = function () { return { done: true, value: undefined }; };
  console.log("K1", it.next().done);
  delete it.next;
  const r = it.next();
  console.log("K2", r.value, r.done);
}

// L: a patch that reaches the builtin through `this` and the prototype.
{
  const s = new Set<number>([3, 4]);
  const it: any = s.values();
  const proto = Object.getPrototypeOf(it);
  it.next = function () { return proto.next.call(this); };
  const got: number[] = [];
  for (const v of it) got.push(v as number);
  console.log("L", got.join(","));
}
