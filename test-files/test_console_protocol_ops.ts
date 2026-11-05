// Which program-visible operations console formatting performs on its
// arguments (wpfacts: the console touch names exactly these).
import { inspect } from "util";

const log: string[] = [];
function rec(s: string): void { log.push(s); }
function flush(label: string): void {
  process.stdout.write(label + " => [" + log.join(", ") + "]\n");
  log.length = 0;
}

function probe(): any {
  return {
    get g() { rec("getter"); return 1; },
    arbitrary() { rec("arbitrary"); return 0; },
    toString() { rec("toString"); return "S"; },
    valueOf() { rec("valueOf"); return 7; },
    toJSON() { rec("toJSON"); return { j: 1 }; },
    [Symbol.toPrimitive](h: string) { rec("toPrimitive:" + h); return 5; },
    [Symbol.iterator]() { rec("iterator"); return [][Symbol.iterator](); },
  };
}
function plainProbe(): any {
  return {
    get g() { rec("getter"); return 1; },
    arbitrary() { rec("arbitrary"); return 0; },
    toString() { rec("toString"); return "S"; },
    valueOf() { rec("valueOf"); return 7; },
    toJSON() { rec("toJSON"); return { j: 1 }; },
  };
}
function customProbe(): any {
  return {
    arbitrary() { rec("arbitrary"); return 0; },
    [Symbol.for("nodejs.util.inspect.custom")](this: any) {
      rec("inspect.custom this=" + (this === undefined ? "undefined" : "obj"));
      return { r: plainProbe() };
    },
  };
}
class Inst {
  x = 1;
  get g(): number { rec("class getter"); return 2; }
  arbitrary(): number { rec("class arbitrary"); return 0; }
  toString(): string { rec("class toString"); return "IS"; }
}
class InstCustom {
  x = 1;
  [inspect.custom](): string { rec("class inspect.custom this=" + (this instanceof InstCustom ? "inst" : "other")); return "IC"; }
}
class Arr extends Array<any> {
  arbitrary(): number { rec("subclass arbitrary"); return 0; }
}
function iterArr(): any[] {
  const a: any[] = [1, 2];
  (a as any)[Symbol.iterator] = function () { rec("array iterator"); return [][Symbol.iterator](); };
  (a as any).arbitrary = function () { rec("array arbitrary"); return 0; };
  return a;
}

const kinds: Array<[string, () => any]> = [
  ["probe", probe],
  ["plain", plainProbe],
  ["custom", customProbe],
  ["inst", () => new Inst()],
  ["instCustom", () => new InstCustom()],
  ["array", iterArr],
  ["subclass", () => { const a = new Arr(); a.push(1); return a; }],
  ["nested", () => ({ inner: probe(), list: [plainProbe()], c: customProbe() })],
  ["error", () => { const e: any = new Error("boom"); e.extra = probe(); e.name = plainProbe(); return e; }],
  ["function", () => { const f: any = function named() {}; f.p = probe(); return f; }],
  ["map", () => new Map<any, any>([[plainProbe(), probe()]])],
  ["set", () => new Set<any>([probe()])],
  ["getterToString", () => ({ x: 1, get toString() { rec("getter toString"); return () => "GS"; } })],
  ["proxy", () => new Proxy(plainProbe(), {
    get(t: any, k: any) { rec("trap get " + String(k)); return t[k]; },
    ownKeys(t: any) { rec("trap ownKeys"); return Reflect.ownKeys(t); },
    getOwnPropertyDescriptor(t: any, k: any) { rec("trap gopd " + String(k)); return Reflect.getOwnPropertyDescriptor(t, k); },
  })],
];

const directives = ["", "%s", "%d", "%i", "%f", "%j", "%o", "%O", "%c"];
for (const [name, make] of kinds) {
  // one argument (the runtime's single-value entry) and a spread rest array
  console.log(make()); flush(name + " log(x)");
  console.error(make()); flush(name + " error(x)");
  console.warn(make()); flush(name + " warn(x)");
  console.info(make()); flush(name + " info(x)");
  console.debug(make()); flush(name + " debug(x)");
  console.log("a", make()); flush(name + " log(a, x)");
  const rest = [make()];
  console.error("m", ...rest); flush(name + " error(m, ...rest)");
  console.table([make()]); flush(name + " table([x])");
  console.dir(make()); flush(name + " dir(x)");
  console.dir(make(), { getters: true, customInspect: true }); flush(name + " dir(x, getters+customInspect)");
  console.assert(false, make()); flush(name + " assert(false, x)");
  console.group(make()); console.groupEnd(); flush(name + " group(x)");
  console.count(make()); flush(name + " count(x)");
  console.time(make()); console.timeEnd(make()); flush(name + " time/timeEnd(x)");
  for (const d of directives) {
    if (d === "") continue;
    console.log(d, make()); flush(name + " log(" + d + ")");
  }
}
