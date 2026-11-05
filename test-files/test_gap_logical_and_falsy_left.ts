// `l && r` is `l` only when `l` is falsy; every object is truthy.
// The left values below reach the result through `&&` and are observed
// by typeof, by a call and by a property read.

function g(c: { x: number }): number {
  return c.x + 1;
}
const ctx = { x: 41 };

const host: any = globalThis as any;
host.u1 = undefined;
host.u2 = g;
const U1: any = host.u1;
const U2: any = host.u2;
console.log(typeof (U1 && g));
console.log((U2 && g)(ctx));
console.log(ctx.x);

const falsy: any[] = [0, -0, NaN, "", false, 0n, null, undefined];
for (const l of falsy) {
  const v: any = l && "right";
  console.log(typeof v, String(v), Object.is(v, -0));
}

const truthy: any[] = [{ a: 1 }, [], function f() {}, "s", 1, -1, 1n, true, Symbol("t"), new Boolean(false)];
for (const l of truthy) {
  const v: any = l && "right";
  console.log(typeof l, typeof v, v === "right");
}

function pick(o: any, r: { y: number }): number {
  const v: any = o && r;
  return v ? v.y : -1;
}
console.log(pick({ y: 100 }, { y: 2 }), pick(null, { y: 3 }), pick(0, { y: 4 }), pick(g, { y: 5 }));

const kept: any = [{ k: 1 }, 0, ""];
for (const l of kept) {
  const v: any = l && { y: 7 };
  console.log(typeof v, v && v.y);
}
