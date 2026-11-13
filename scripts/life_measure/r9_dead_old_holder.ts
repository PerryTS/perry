// LIFE I3f fixture: a module-level pool of holder objects is promoted (the runtime's JSON.parse
// allocates about 1 MB per call, six calls before run). Each iteration of run allocates one object,
// stores it into its pool holder (an old object: the store goes through the write barrier, the
// holder enters the remembered set) and drops the holder from the pool: the holder is dead but not
// collected, and nothing else reaches the new object. Clean per Node semantics at every density.
// A minor treats every old object as alive, so its reach through the remembered set alone would
// make each iteration dirty (the `lifeminor` control keeps that verdict and must fail).
type Holder = { x: { v: number } | null };
const pool: (Holder | null)[] = [];
for (let i = 0; i < 40; i++) pool.push({ x: null });
function churn(src: string): number {
  let t = 0;
  for (let k = 0; k < 6; k++) {
    const a: { v: number }[] = JSON.parse(src);
    t += a.length;
  }
  return t;
}
function run(n: number): number {
  let t = 0;
  for (let i = 0; i < n; i++) {
    const o = { v: i };
    const h = pool[i] as Holder;
    h.x = o;
    pool[i] = null;
    t += o.v;
  }
  return t;
}
const parts: string[] = [];
for (let j = 0; j < 20000; j++) parts.push('{"v":' + j + "}");
console.log(churn("[" + parts.join(",") + "]"));
console.log(run(40));
let dropped = 0;
for (let i = 0; i < pool.length; i++) if (pool[i] === null) dropped++;
console.log(dropped);
