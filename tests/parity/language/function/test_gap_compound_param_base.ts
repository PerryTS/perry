// A compound member assignment `a[k] op= v` whose receiver is a parameter
// reads the receiver ONCE, before the key and the right-hand side. A
// parameter that nothing can rebind is used directly as the receiver; any
// parameter that can be rebound keeps a snapshot. Every case below prints
// what node prints. Compared byte-for-byte against
// `node --experimental-strip-types`.

const log: string[] = [];

// Call order with a never-assigned parameter: key, ToPropertyKey, getter,
// right-hand side, setter.
function traced(name: string) {
  let v = 10;
  return {
    get x() { log.push(name + ".get"); return v; },
    set x(n: number) { log.push(name + ".set " + n); v = n; },
  };
}
function key(k: string) {
  return { toString() { log.push("key " + k); return k; } };
}
function rhs(n: number) { log.push("rhs " + n); return n; }

function neverAssigned(o: any) {
  o[key("x") as any] += rhs(1);
  o.x -= rhs(2);
  o[key("x") as any] ||= rhs(3);
  o[key("x") as any] &&= rhs(4);
  o[key("x") as any] ??= rhs(5);
  return o.x;
}
log.push("result " + neverAssigned(traced("o")));
console.log(log.join("\n"));
log.length = 0;

// A Proxy receiver sees exactly one get and one set per statement.
function proxied(p: any, i: number) {
  for (let j = 0; j < 3; j++) p[i + j] += j;
}
const target: number[] = [1, 2, 3, 4];
const p = new Proxy(target, {
  get(t: any, k: any) { log.push("get " + String(k)); return t[k]; },
  set(t: any, k: any, v: any) { log.push("set " + String(k) + "=" + v); t[k] = v; return true; },
});
proxied(p, 1);
console.log(log.join(" "), target.join(","));
log.length = 0;

// The right-hand side reassigns the parameter: the OLD receiver is written.
function reassignedInRhs(a: number[], b: number[]) {
  a[0] += ((a = b), 100);
  return a;
}
{
  const x = [1, 2], y = [5, 6];
  const r = reassignedInRhs(x, y);
  console.log("rhs:", x.join(","), y.join(","), r === y);
}

// The key reassigns the parameter.
function reassignedInKey(a: number[], b: number[]) {
  a[((a = b), 1)] *= 10;
  return a;
}
{
  const x = [1, 2], y = [5, 6];
  reassignedInKey(x, y);
  console.log("key:", x.join(","), y.join(","));
}

// A closure called from the right-hand side reassigns the parameter.
function reassignedByClosure(a: { n: number }, b: { n: number }) {
  const swap = () => { a = b; return 1; };
  a.n += swap();
  a.n += 1000;
  return a;
}
{
  const x = { n: 1 }, y = { n: 50 };
  reassignedByClosure(x, y);
  console.log("closure:", x.n, y.n);
}

// A closure from a parameter default reassigns the parameter.
function reassignedByDefault(a: { n: number }, b: { n: number }, swap = () => { a = b; return 1; }) {
  a.n += swap();
  return a.n;
}
{
  const x = { n: 1 }, y = { n: 50 };
  console.log("default:", reassignedByDefault(x, y), x.n, y.n);
}

// Reassigned later in a loop: each iteration's compound writes the current
// receiver.
function reassignedInLoop(a: number[], rows: number[][]) {
  for (let r = 0; r < rows.length; r++) {
    a[0] += r + 1;
    a = rows[r];
  }
}
{
  const rows = [[0], [0], [0]];
  const first = [0];
  reassignedInLoop(first, rows);
  console.log("loop:", first[0], rows.map((r) => r[0]).join(","));
}

// A never-assigned parameter used inside a closure, called repeatedly.
function viaClosure(acc: Float64Array) {
  const add = (i: number, v: number) => { acc[i * 2] += v; acc[i * 2 + 1] -= v; };
  for (let i = 0; i < 4; i++) add(i, i + 0.5);
  return Array.from(acc).join(",");
}
console.log("closure-param:", viaClosure(new Float64Array(8)));

// The #11810 kernel shape: a module-level typed array passed in, a spilled
// computed key, every compound op.
const D = new Float64Array(14);
function advance(a: Float64Array, dt: number) {
  for (let i = 0; i < 2; i++) {
    const oi = i * 7;
    a[oi + 3] -= dt * 2;
    a[oi + 4] += dt;
    a[oi + 5] *= 1.5;
    a[oi + 6] /= 2;
    a[oi] += a[oi + 3] * dt;
  }
}
for (let i = 0; i < D.length; i++) D[i] = i + 1;
for (let s = 0; s < 5; s++) advance(D, 0.25);
console.log("advance:", Array.from(D).map((v) => v.toFixed(6)).join(","));
