// Every fact a function object's JsFunctionInfo carries, observed through
// JS: the parameter count callers pad to, the rest kind and its fixed
// prefix, `.length` and its declared fallback, the arrow / async /
// generator / async-generator / non-constructor bits, and the compiled
// direct-call clones (a captured arrow called from a loop and from an
// array method). Each fact is read through a GENERIC call (the function
// travels as a value) so the runtime reads it from the info, not from a
// statically known callee.

const out: string[] = [];
const show = (label: string, v: unknown) => out.push(label + " " + JSON.stringify(v));
const tryNew = (f: any) => {
  try {
    new f();
    return "constructed";
  } catch (e) {
    return (e as Error).constructor.name;
  }
};

const values: any[] = [];
function keep<T>(f: T): T {
  values.push(f);
  return values[values.length - 1];
}

// params: a caller passing fewer pads with undefined to the declared count.
const three = keep(function three(a: any, b: any, c: any) {
  return [a, b === undefined, c === undefined];
});
show("pad", three(1));
show("pad.call", three.call(null, 1, 2));
show("pad.apply", three.apply(null, [1]));
show("pad.map", [7].map(three as any));

// rest: the fixed prefix and the bundled tail.
const rest = keep(function rest(a: any, b: any, ...xs: any[]) {
  return [a, b, xs];
});
show("rest0", rest());
show("rest1", rest(1));
show("rest4", rest(1, 2, 3, 4));
show("rest.apply", rest.apply(null, [1, 2, 3]));
const restOnly = keep((...xs: any[]) => xs.length);
show("restOnly", [restOnly(), restOnly(1, 2, 3), restOnly.call(null, 1)]);

// arguments: every argument, fixed parameters included.
const args = keep(function args(a: any) {
  return [a, arguments.length, Array.prototype.slice.call(arguments)];
});
show("arguments", args(1, 2, 3));
show("arguments0", args());
const both = keep(function both(a: any, ...r: any[]) {
  return [arguments.length, r.length, a];
});
show("rest+arguments", both(1, 2, 3, 4));

// .length and its declared fallback.
show("length", [
  three.length,
  rest.length,
  restOnly.length,
  args.length,
  keep(function (a: any, b = 1, c?: any) {}).length,
  keep((x: any, y: any) => 0).length,
  keep(async (x: any) => 0).length,
  keep(function* (p: any, q: any) {}).length,
  three.bind(null, 1).length,
  rest.bind(null).length,
]);
show("builtin length", [Math.max.length, [].map.length, Array.prototype.push.length, JSON.stringify.length, Object.keys.length]);

// kind bits: arrow, async, generator, async generator, builtin. (A concise
// method's missing [[Construct]] and the async/generator toStringTag are
// separate gaps on main and not read from here.)
const arrow = keep(() => 1);
const asyncFn = keep(async function () {});
const gen = keep(function* () {});
const asyncGen = keep(async function* () {});
show("new", [
  tryNew(keep(function F(this: any) { this.x = 1; })),
  tryNew(arrow),
  tryNew(asyncFn),
  tryNew(gen),
  tryNew(asyncGen),
  tryNew(Math.max),
]);
show("prototype", [
  typeof (three as any).prototype,
  typeof (arrow as any).prototype,
  typeof (asyncFn as any).prototype,
  typeof (gen as any).prototype,
]);
show("gen", Array.from((gen as any)()).length);

// strict receiver: a plain call's `this` is undefined, a primitive
// receiver stays primitive.
const recv = keep(function (this: any) {
  return typeof this;
});
show("this", [recv(), recv.call(5), recv.call("s"), recv.call({})]);
// arrows keep the lexical receiver whatever the caller passes.
const holder = {
  tag: "h",
  make() {
    return keep(() => (this as any).tag);
  },
};
const lexical = holder.make();
show("arrow this", [lexical(), lexical.call({ tag: "other" })]);

// the direct-call clones: a capturing arrow run from a loop and from array
// methods, with a boxed (reassigned) capture and a plain one.
let boxed = 0;
const step = 3;
const add = keep((x: number) => {
  boxed += x * step;
  return boxed;
});
let last = 0;
for (let i = 0; i < 1000; i++) last = add(i);
show("loop clone", [last, boxed]);
show("map clone", [1, 2, 3].map(add));
show("reduce clone", [1, 2, 3, 4].reduce((acc, x) => acc + x * step, 0));
show("filter clone", [1, 2, 3, 4, 5, 6].filter((x) => x % step === 0));

for (const line of out) console.log(line);
