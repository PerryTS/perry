// N-ary Math.min/Math.max: ToNumber order, NaN, signed zero, arity 0/1/2/3/5.
const log: string[] = [];
function obj(name: string, v: number): any {
  return { valueOf() { log.push(name); return v; } };
}
function show(x: number): string {
  return Object.is(x, -0) ? "-0" : String(x);
}
function p(label: string, x: number) {
  console.log(label, show(x));
}
const nan = NaN;
const a: number = Number(process.argv[2] ?? "3");
const b: number = a + 1;
const c: number = a - 5;
const d: number = a * 2;
const e: number = a / 2;

p("min0", Math.min());
p("max0", Math.max());
p("min1", Math.min(a));
p("max1", Math.max(c));
p("min1 nan", Math.min(nan));
p("min2", Math.min(a, b));
p("max2", Math.max(a, b));
p("min3", Math.min(a, b, c));
p("max3", Math.max(a, b, c));
p("min5", Math.min(a, b, c, d, e));
p("max5", Math.max(a, b, c, d, e));
p("min3 nan first", Math.min(nan, a, b));
p("min3 nan mid", Math.min(a, nan, b));
p("min3 nan last", Math.min(a, b, nan));
p("max3 nan first", Math.max(nan, a, b));
p("max3 nan mid", Math.max(a, nan, b));
p("max3 nan last", Math.max(a, b, nan));

const z0 = Number(process.argv[3] ?? "0");
const nz = -z0;
p("min -0,+0,x", Math.min(nz, z0, a));
p("min +0,-0,x", Math.min(z0, nz, a));
p("min x,+0,-0", Math.min(a, z0, nz));
p("min +0,x,-0", Math.min(z0, a, nz));
p("max -0,+0,x0", Math.max(nz, z0, c));
p("max +0,-0,c", Math.max(z0, nz, c));
p("max c,-0,+0", Math.max(c, nz, z0));
p("max -0,c,+0", Math.max(nz, c, z0));
p("max -0,-0,-0", Math.max(nz, nz, nz));
p("min +0,+0,+0", Math.min(z0, z0, z0));
p("min 5 zeros", Math.min(z0, z0, nz, z0, z0));
p("max 5 zeros", Math.max(nz, nz, z0, nz, nz));

log.length = 0;
p("obj min3", Math.min(obj("x", 3), obj("y", 1), obj("z", 2)));
console.log(log.join(","));
log.length = 0;
p("obj max5", Math.max(obj("a", 3), obj("b", 1), obj("c", 7), obj("d", 2), obj("e", 4)));
console.log(log.join(","));
log.length = 0;
p("obj nan mid", Math.min(obj("a", 1), obj("b", NaN), obj("c", 0)));
console.log(log.join(","));
log.length = 0;
p("mixed", Math.max(a, obj("m", 100), "42" as any, null as any, true as any));
console.log(log.join(","));
log.length = 0;
p("str undefined", Math.min("7" as any, undefined as any, 2));
p("str", Math.min("7" as any, "2" as any, 9));

// arguments are all evaluated before any is coerced
log.length = 0;
function ev(name: string, v: any) { log.push("eval-" + name); return v; }
const r = Math.max(ev("1", obj("v1", 1)), ev("2", obj("v2", 2)), ev("3", obj("v3", 3)));
p("order", r);
console.log(log.join(","));

// bigint: earlier arguments coerced, later ones are not
log.length = 0;
try {
  Math.min(obj("p", 1), obj("q", 2), 10n as any, obj("r", 3));
  console.log("no throw");
} catch (err: any) {
  console.log(err instanceof TypeError ? "TypeError" : "other", log.join(","));
}
log.length = 0;
try {
  Math.max(a, 1n as any, obj("later", 3));
  console.log("no throw");
} catch (err: any) {
  console.log(err instanceof TypeError ? "TypeError" : "other", log.join(","));
}

// spread keeps working
const arr = [a, b, c];
p("spread min", Math.min(...arr));
p("spread max", Math.max(...arr));

// fixed-arity out-of-line helpers (2, 3, 4 operands) vs the N-ary one (5)
log.length = 0;
p("obj min4", Math.min(obj("a", 3), obj("b", 1), obj("c", 7), obj("d", 2)));
console.log(log.join(","));
log.length = 0;
p("obj max4 nan", Math.max(obj("a", 3), obj("b", NaN), obj("c", 7), obj("d", 2)));
console.log(log.join(","));
log.length = 0;
p("obj min2", Math.min(obj("a", 3), obj("b", -0)));
console.log(log.join(","));
log.length = 0;
p("obj max2 zeros", Math.max(obj("a", -0), obj("b", 0)));
console.log(log.join(","));
log.length = 0;
try {
  Math.max(obj("p", 1), 10n as any, obj("q", 3), obj("r", 4));
  console.log("no throw");
} catch (err: any) {
  console.log(err instanceof TypeError ? "TypeError" : "other", log.join(","));
}
log.length = 0;
try {
  Math.min(obj("p", 1), 10n as any);
  console.log("no throw");
} catch (err: any) {
  console.log(err instanceof TypeError ? "TypeError" : "other", log.join(","));
}
p("min4 mixed", Math.min("7" as any, a, null as any, undefined as any));
p("max4 mixed", Math.max("7" as any, a, null as any, true as any));
p("min4 zeros", Math.min(z0 as any, nz as any, "0" as any, z0));
