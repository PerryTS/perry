// #14732: a shorthand property in an object ASSIGNMENT pattern applies its
// default (`({ o = 7 } = {})`), like the declaration form does. The default is
// evaluated only when the read is undefined, and an anonymous function default
// is named after its target.

const out: string[] = [];
function log(label: string, value: unknown): void {
  out.push(label + "=" + String(value));
}

let o: any;
({ o = 7 } = {});
log("shorthand", o);
({ o = 7 } = { o: 1 });
log("present", o);
({ o = 7 } = { o: null });
log("null", o);
({ o = 7 } = { o: NaN });
log("nan", o);
({ o = 7 } = { o: 0 });
log("zero", o);

let q: any, r: any;
[[q = 1], { r = 2 }] = [[], {}];
log("nested", q + ":" + r);
({ x: { r = 3 } } = { x: {} } as any);
log("nested-object", r);

let b: any;
({ a: b = 3 } = {} as any);
log("renamed", b);

const key = "k";
let c: any, d: any;
({ [key]: c = 4, d = 5 } = {} as any);
log("computed", c + ":" + d);

let e: any, rest: any;
({ e = 1, ...rest } = { f: 2, g: 3 } as any);
log("rest", e + ":" + JSON.stringify(rest));

let calls = 0;
function next(): number {
  calls += 1;
  return 100 + calls;
}
let s: any, t: any;
({ s = next(), t = next() } = { s: "given" } as any);
log("side-effects", s + ":" + t + ":" + calls);

let first: any, second: any;
({ first = 1, second = first + 1 } = {} as any);
log("earlier", first + ":" + second);

let fn: any;
({ fn = function () {} } = {} as any);
log("fn-name", fn.name);
let arrow: any;
({ arrow = () => 0 } = {} as any);
log("arrow-name", arrow.name);

function inFunction(src: any): string {
  let p: any, w: any;
  ({ p = "dp", w = "dw" } = src);
  return p + ":" + w;
}
log("function", inFunction({ w: "w" }));

for (const line of out) console.log(line);
