// The array element read guards on ONE header word (an ordinary array, not a
// growth stub, no element descriptors), bounds the index by capacity, and
// consults the prototype only on the hole / out-of-bounds arm. Every case here
// is a fact that guard leaves to another arm: each must still answer as node.
// Prototype-polluting cases run LAST, because their protector is sticky.

function read(xs: any[], i: number): any {
  return xs[i];
}

function readLoop(xs: any[], n: number): string {
  let out = "";
  for (let i = 0; i < n; i++) out += String(xs[i]) + ",";
  return out;
}

// 1. A slot vacated by pop / splice / shift / length= reads as undefined: the
//    bound is capacity, so the vacated slot must hold the hole, not the value.
{
  const a: any[] = [{ v: 1 }, { v: 2 }, { v: 3 }];
  a.pop();
  console.log("pop", a.length, read(a, 2), read(a, 1).v);
  const b: any[] = ["p", "q", "r", "s"];
  b.splice(1, 2);
  console.log("splice", b.length, read(b, 2), read(b, 3), readLoop(b, 4));
  const c: any[] = ["x", "y", "z"];
  c.shift();
  console.log("shift", c.length, read(c, 2), readLoop(c, 3));
  const d: any[] = [1, 2, 3, 4, 5];
  d.length = 2;
  console.log("length=", read(d, 2), read(d, 4), readLoop(d, 5));
  const e: any[] = [10, 20, 30];
  e.pop();
  e.length = 3;
  console.log("pop+grow", read(e, 2), e[2] === undefined);
}

// 2. A stale alias of an array that grew: the old head is a forwarded stub,
//    which fails the guard word and is followed on the cold arm.
{
  const a: any[] = [0, 1];
  const alias: any[] = a;
  for (let i = 2; i < 200; i++) a.push(i);
  let sum = 0;
  for (let i = 0; i < 200; i++) sum += read(alias, i);
  console.log("stale alias", read(alias, 150), sum, alias.length);
}

// 3. Holes and out-of-bounds with a clean prototype: undefined.
{
  const h: any[] = [1, , 3];
  console.log("hole", read(h, 1), read(h, 3), read(h, 1000), readLoop(h, 4));
  const n: any[] = [1, 2];
  (n as any)["-1"] = "negative";
  console.log("negative index", read(n, -1), read(n, -2));
}

// 4. An over-long `new Array(n)`: a sparse index past the backing store but
//    below length is an own property the inline read must not answer.
{
  const big: any[] = new Array(1_200_000);
  big[5] = "five";
  big[1_150_000] = "far";
  console.log("overlong", read(big, 5), read(big, 6), read(big, 1_150_000), read(big, 1_100_000), read(big, 1_300_000), big.length);
}

// 5. Receivers that are not ordinary arrays behind an `any[]` claim.
{
  const t: any = new Float64Array([1.5, 2.5, 3.5]);
  console.log("typed array", read(t, 1), read(t, 3));
  class MyArr extends Array<any> {}
  const m = new MyArr();
  m.push("m0", "m1");
  console.log("subclass", read(m as any, 1), read(m as any, 2));
  const o: any = { 0: "zero", 1: "one", length: 2 };
  console.log("array-like", read(o, 1), read(o, 2));
  const s: any = "str";
  console.log("string", read(s, 1), read(s, 5));
}

// 6. An accessor element: descriptors are in the guard word.
{
  const acc: any[] = [1, 2, 3];
  let calls = 0;
  Object.defineProperty(acc, 1, { get() { calls++; return "getter"; } });
  console.log("accessor", read(acc, 1), read(acc, 0), calls);
  const frozen: any[] = Object.freeze(["f0", "f1"]) as any[];
  console.log("frozen", read(frozen, 1), read(frozen, 2));
}

// 7. setPrototypeOf on one array: its own custom-prototype bit.
{
  const p: any[] = [];
  Object.setPrototypeOf(p, { 2: "from proto", length: 0 });
  p[0] = "own0";
  console.log("setPrototypeOf", read(p, 0), read(p, 2), read(p, 5));
}

// 8. Index properties on Array.prototype and Object.prototype: holes and
//    out-of-bounds reads now reach the prototype (the protector latches).
{
  const h: any[] = [1, , 3, , ];
  Object.defineProperty(Array.prototype, 3, { get() { return "AP3"; }, configurable: true });
  // Read while the getter is installed, print after it is gone: node's own
  // console.log builds arrays that trip over it.
  const r1 = [read(h, 1), read(h, 3), read(h, 0), h.length].join("|");
  const vacated: any[] = ["a", "b", "c", "d"];
  vacated.pop();
  const r2 = [read(vacated, 3), vacated.length].join("|");
  delete (Array.prototype as any)[3];
  console.log("Array.prototype getter", r1);
  console.log("vacated slot sees the prototype", r2);
  (Object.prototype as any)[1] = "OP1";
  const g: any[] = [7, , 9];
  console.log("Object.prototype index", read(g, 1), read(g, 0), read([], 1));
  delete (Object.prototype as any)[1];
  console.log("after delete", read(g, 1));
}
