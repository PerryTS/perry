// A loop that pushes into an array may carry the push admission from one
// iteration to the next, but only until something could have changed the
// array or its prototypes. Every case below changes that state in the middle
// of a pushing loop and checks the next push sees it.

function attempt(label: string, f: () => unknown): void {
  try {
    console.log(label, JSON.stringify(f()));
  } catch (e) {
    console.log(label, "throws", (e as Error).constructor.name);
  }
}

function freezeIn(xs: number[]): void {
  Object.freeze(xs);
}

// Frozen / sealed / non-extensible by a callee in the loop body.
for (const lock of ["freeze", "seal", "preventExtensions"]) {
  const xs: number[] = [];
  attempt(lock + "-in-body", () => {
    for (let i = 0; i < 10; i++) {
      xs.push(i);
      if (i === 4) {
        if (lock === "freeze") freezeIn(xs);
        else if (lock === "seal") Object.seal(xs);
        else Object.preventExtensions(xs);
      }
    }
    return xs.length;
  });
  console.log(lock, xs.length, xs[4], 5 in xs);
}

// Frozen by the pushed value's own getter, after the receiver was admitted on
// the previous iteration.
{
  const xs: number[] = [];
  let n = 0;
  const src = {
    get v(): number {
      n++;
      if (n === 6) Object.freeze(xs);
      return n;
    },
  };
  attempt("getter-freeze", () => {
    for (let i = 0; i < 10; i++) xs.push(src.v);
    return xs.length;
  });
  console.log("getter-freeze", xs.length, n);
}

// Frozen from the loop condition (reached over the back edge).
{
  const ys: number[] = [];
  let i = 0;
  const bound = {
    valueOf(): number {
      if (i === 3) Object.freeze(ys);
      return 8;
    },
  };
  attempt("condition-freeze", () => {
    for (; i < (bound as unknown as number); i++) ys.push(i);
    return ys.length;
  });
  console.log("condition-freeze", ys.length, i);
}

// A non-writable length installed mid-loop.
{
  const zs: number[] = [];
  attempt("length-readonly", () => {
    for (let i = 0; i < 6; i++) {
      zs.push(i);
      if (i === 2) Object.defineProperty(zs, "length", { writable: false });
    }
    return zs.length;
  });
  console.log("length-readonly", zs.length);
}

// A custom prototype with an indexed setter, installed mid-loop.
{
  const log: number[] = [];
  const proto = Object.create(Array.prototype, {
    3: {
      set(v: number) {
        log.push(v);
      },
      configurable: true,
    },
  });
  const qs: number[] = [];
  for (let i = 0; i < 6; i++) {
    if (i === 1) Object.setPrototypeOf(qs, proto);
    qs.push(i + 100);
  }
  console.log("custom-proto", qs.length, JSON.stringify(log), 3 in qs);
}

// The binding is rebound to another array without a call.
{
  const first: number[] = [];
  const second: number[] = [9];
  let cur = first;
  for (let i = 0; i < 40; i++) {
    cur.push(i);
    if (i === 20) cur = second;
  }
  console.log("rebind", first.length, second.length, first[20], second[1], second[19]);
}

// Two names for one array, pushed in the same loop, with inline pops.
{
  const a: number[] = [];
  const b = a;
  let popped = 0;
  for (let i = 0; i < 50; i++) {
    a.push(i);
    b.push(-i);
    if (i % 7 === 0) popped += b.pop() ?? 0;
  }
  console.log("aliases", a.length, b.length, a === b, popped, a[a.length - 1], a[10]);
}

// Nested loops into one array, across several growths.
{
  const out: number[] = [];
  let k = 0;
  while (k < 5) {
    for (let j = 0; j < 30; j++) out.push(k * 100 + j);
    out.push(-1);
    k++;
  }
  let d = 0;
  do {
    out.push(d);
    d++;
  } while (d < 20);
  let sum = 0;
  for (const v of out) sum += v;
  console.log("nested", out.length, out[29], out[30], out[31], out[out.length - 1], sum);
}

// A generator that yields between pushes.
function* filler(xs: number[]): Generator<number> {
  for (let i = 0; i < 25; i++) {
    xs.push(i);
    yield xs.length;
  }
}
{
  const gs: number[] = [];
  let last = 0;
  for (const n of filler(gs)) {
    last = n;
    if (n === 10) gs.push(-5);
  }
  console.log("generator", gs.length, last, gs[10], gs[24]);
}

// Objects and strings pushed in a memo'd loop, then read back.
{
  const objs: { id: number }[] = [];
  const strs: string[] = [];
  for (let i = 0; i < 70; i++) {
    objs.push({ id: i });
    strs.push("s" + i);
  }
  let total = 0;
  for (const o of objs) total += o.id;
  console.log("pointers", objs.length, total, strs[69], strs.join("").length);
}
