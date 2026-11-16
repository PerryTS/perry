// An extra argument that throws, and a used argument whose ToNumber throws before a later one.
const k = Number(process.argv[2] ?? "0");
const log: string[] = [];
const boom = (): any => { log.push("boom"); throw new Error("boom"); };
const t = { valueOf(): number { log.push("t"); throw new Error("t"); } };
const o = (name: string): any => ({ valueOf() { log.push(name); return k; } });
function run(f: () => unknown): void {
  try { console.log("ok", f()); } catch (err) { console.log("threw", (err as Error).message); }
  console.log(log.join(",")); log.length = 0;
}
run(() => Math.cos(k, boom()));
run(() => Math.atan2(k, 1, boom()));
run(() => Math.cos(o("a"), boom()));
run(() => Math.atan2(t as any, o("b")));
run(() => Math.max(o("a"), t as any, o("c")));
run(() => Math.hypot(t as any, boom()));
run(() => Math.pow(o("a"), t as any, boom()));
