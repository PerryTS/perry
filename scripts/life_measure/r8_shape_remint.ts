// LIFE I3d fixture: each iteration of run creates an Error-subclass instance (zod's `ZodError`
// pattern: own `stack`/`message`/`name`/`issues` keys with attributes, minted by the runtime as
// private key and attribute lists), keeps it alive across natural minor collections (promotion;
// the runtime's JSON.parse allocates about 1 MB per iteration, no compiled loop inside), then
// drops it. Only engine key lists (shape metadata, never a JS value) can outlive an iteration,
// held by the shape table. Clean per Node semantics at every density. Measured (I3e): dense
// meta_objs 5, and the `lifemeta` control counts those 5 lists as survivors (one dirty iteration).
class E extends Error {
  issues: number[];
  constructor(m: string, k: number) {
    super(m);
    this.name = "E";
    this.issues = [k];
  }
}
function run(n: number, src: string): number {
  let t = 0;
  for (let i = 0; i < n; i++) {
    const e = new E("m" + i, i);
    const tmp: { v: number }[] = JSON.parse(src);
    t += e.issues[0] + tmp.length;
  }
  return t;
}
const parts: string[] = [];
for (let j = 0; j < 20000; j++) parts.push('{"v":' + j + "}");
console.log(run(40, "[" + parts.join(",") + "]"));
