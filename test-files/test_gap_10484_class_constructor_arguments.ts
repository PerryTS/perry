// #10484: `arguments` inside a class CONSTRUCTOR must describe the call site.
//
// Two defects, one object:
//   - a static `new C(x)` reported the DECLARED parameter count, because the
//     call site was padded with `undefined` up to the declared arity before
//     the arguments list was packed;
//   - construction through a runtime value (`const K = C; new K(x)`, a class
//     returned from a function, an imported or CommonJS class,
//     `Reflect.construct`) reported an EMPTY list, because the dynamic
//     construct path bound the synthesized `arguments` slot like a user rest
//     parameter (only the arguments past the declared ones).
//
// undici 8.9.0's `new Request(url)` (webidl `argumentLengthCheck(arguments, 1)`)
// and whatwg-url's `new URL(href)` (a class declared inside `install()` that
// checks `arguments.length < 1`) both threw "1 argument required, but 0 found".
import cjs from "./fixtures/issue_10484_ctor_arguments/request.cjs";
import * as esm from "./fixtures/issue_10484_ctor_arguments/classes.ts";
import { ExpTwo, ExpDefault, fmtArgs } from "./fixtures/issue_10484_ctor_arguments/classes.ts";

function row(label: string, f: () => any): void {
  try {
    const o = f();
    console.log(label, "=>", o.r ?? o.n ?? o.argc ?? o.href ?? o.url);
  } catch (e: any) {
    console.log(label, "=> threw", e.constructor.name, e.message);
  }
}

// ── module-level classes ──
class Two {
  r: string;
  constructor(p?: any, q?: any) {
    this.r = fmtArgs(arguments);
  }
}
class NoParams {
  r: string;
  constructor() {
    this.r = fmtArgs(arguments);
  }
}
class WithDefault {
  r: string;
  constructor(p: any, q: any = "dq") {
    this.r = fmtArgs(arguments) + " q=" + q;
  }
}
class WithRest {
  r: string;
  constructor(first?: any, ...rest: any[]) {
    this.r = fmtArgs(arguments) + " rest=" + JSON.stringify(rest);
  }
}
class LengthOnly {
  n: number;
  constructor(p?: any, q?: any) {
    this.n = arguments.length;
  }
}
class Indexed {
  r: string;
  constructor(p?: any) {
    this.r = [arguments[0], arguments[1], arguments[2]].map(String).join(",");
  }
}
class WithField {
  tag = "field";
  p: any;
  r: string;
  constructor(p?: any) {
    this.p = p;
    this.r = fmtArgs(arguments) + " " + this.tag + " p=" + this.p;
  }
}

console.log("-- static new --");
row("Two()", () => new Two());
row("Two(a)", () => new Two("a"));
row("Two(a,b)", () => new Two("a", "b"));
row("Two(a,b,c)", () => new Two("a", "b", "c"));
row("Two(undefined)", () => new Two(undefined));
row("Two(a,undefined)", () => new Two("a", undefined));
row("NoParams()", () => new NoParams());
row("NoParams(a,b)", () => new NoParams("a", "b"));
row("WithDefault(a)", () => new WithDefault("a"));
row("WithDefault(a,b,c)", () => new WithDefault("a", "b", "c"));
row("WithRest()", () => new WithRest());
row("WithRest(a)", () => new WithRest("a"));
row("WithRest(a,b,c)", () => new WithRest("a", "b", "c"));
row("LengthOnly()", () => new LengthOnly());
row("LengthOnly(a)", () => new LengthOnly("a"));
row("LengthOnly(a,b,c)", () => new LengthOnly("a", "b", "c"));
row("Indexed(a)", () => new Indexed("a"));
row("Indexed(a,b,c)", () => new Indexed("a", "b", "c"));
row("WithField(a)", () => new WithField("a"));
row("WithField()", () => new WithField());

console.log("-- spread and Reflect.construct --");
const none: any[] = [];
const one: any[] = ["s1"];
const three: any[] = ["s1", "s2", "s3"];
row("Two(...[])", () => new Two(...none));
row("Two(...[1])", () => new Two(...one));
row("Two(...[3])", () => new Two(...three));
row("WithRest(...[3])", () => new WithRest(...three));
row("Reflect.construct(Two,[])", () => Reflect.construct(Two, []));
row("Reflect.construct(Two,[1])", () => Reflect.construct(Two, ["x"]));
row("Reflect.construct(Two,[3])", () => Reflect.construct(Two, ["x", "y", "z"]));
row("Reflect.construct(WithDefault,[1])", () => Reflect.construct(WithDefault, ["x"]));
row("Reflect.construct(LengthOnly,[1])", () => Reflect.construct(LengthOnly, ["x"]));

console.log("-- class stored in a variable --");
const V: any = Two;
const VD: any = WithDefault;
const VR: any = WithRest;
const VL: any = LengthOnly;
const VN: any = NoParams;
row("V()", () => new V());
row("V(a)", () => new V("a"));
row("V(a,b,c)", () => new V("a", "b", "c"));
row("V(...[3])", () => new V(...three));
row("VD(a)", () => new VD("a"));
row("VR(a,b,c)", () => new VR("a", "b", "c"));
row("VL(a)", () => new VL("a"));
row("VN(a,b)", () => new VN("a", "b"));
const table: Record<string, any> = { Two, WithField };
row("table.Two(a)", () => new table.Two("a"));
row("table[WithField](a)", () => new table["WithField"]("a"));

console.log("-- class returned from / declared inside a function --");
function makeInner() {
  return class Inner {
    r: string;
    constructor(p?: any) {
      this.r = fmtArgs(arguments);
    }
  };
}
const Inner = makeInner();
row("Inner()", () => new Inner());
row("Inner(a)", () => new Inner("a"));
row("Inner(a,b)", () => new (Inner as any)("a", "b"));

function makeCapturing(suffix: string) {
  class Local {
    r: string;
    constructor(p?: any, q?: any) {
      this.r = fmtArgs(arguments) + " " + suffix;
    }
  }
  const direct = new Local("inside");
  console.log("Local(inside) static =>", direct.r);
  return Local;
}
const Local: any = makeCapturing("captured");
row("Local(a)", () => new Local("a"));
row("Local(a,b,c)", () => new Local("a", "b", "c"));

// whatwg-url shape: class declared inside `install`, arity checked by hand.
function install(globalObject: any) {
  const prefix = "Failed to construct 'URL': ";
  class URL {
    href: string;
    constructor(url: any) {
      if (arguments.length < 1) {
        throw new TypeError(prefix + "1 argument required, but only " + arguments.length + " present.");
      }
      const args: string[] = [];
      {
        const curArg = arguments[0];
        args.push(String(curArg));
      }
      {
        const curArg = arguments[1];
        if (curArg !== undefined) args.push(String(curArg));
      }
      this.href = args.join(" @ ");
    }
  }
  globalObject.URL = URL;
}
const exportsObj: any = {};
install(exportsObj);
row("URL(href)", () => new exportsObj.URL("http://a/"));
row("URL(href,base)", () => new exportsObj.URL("/p", "http://b/"));
row("URL()", () => new exportsObj.URL());

console.log("-- subclasses --");
class SpreadArgs extends Two {
  constructor(x?: any) {
    super(...arguments);
  }
}
class NoCtor extends Two {}
class Explicit extends Two {
  d: string;
  constructor(x?: any, y?: any, z?: any) {
    super(x);
    this.d = fmtArgs(arguments);
  }
}
class RestForward extends Two {
  constructor(...args: any[]) {
    super(...args);
  }
}
row("SpreadArgs(a)", () => new SpreadArgs("a"));
row("SpreadArgs(a,b,c)", () => new (SpreadArgs as any)("a", "b", "c"));
row("NoCtor()", () => new NoCtor());
row("NoCtor(a)", () => new NoCtor("a"));
row("NoCtor(a,b,c)", () => new (NoCtor as any)("a", "b", "c"));
row("Explicit(a,b) base", () => new Explicit("a", "b"));
console.log("Explicit(a,b) own =>", new Explicit("a", "b").d);
row("RestForward(a)", () => new RestForward("a"));
row("RestForward(a,b,c)", () => new RestForward("a", "b", "c"));
const VS: any = SpreadArgs;
const VNC: any = NoCtor;
row("VS(a,b)", () => new VS("a", "b"));
row("VNC(a)", () => new VNC("a"));
row("VNC(a,b,c)", () => new VNC("a", "b", "c"));
const nt: any = Reflect.construct(Two, ["n1"], RestForward);
console.log("Reflect.construct(Two,[1],RestForward) =>", nt.r, nt instanceof RestForward);
class FromValue extends (V as any) {
  constructor(a?: any) {
    super(a, "extra");
  }
}
row("FromValue(a)", () => new FromValue("a"));

console.log("-- imported ESM classes --");
row("ExpTwo()", () => new ExpTwo());
row("ExpTwo(a)", () => new ExpTwo("a"));
row("ExpTwo(a,b,c)", () => new (ExpTwo as any)("a", "b", "c"));
row("ExpDefault(a)", () => new ExpDefault("a"));
row("esm.ExpTwo(a)", () => new esm.ExpTwo("a"));
row("esm.ExpRest(a,b,c)", () => new esm.ExpRest("a", "b", "c"));
row("esm.ExpLength(a)", () => new esm.ExpLength("a"));
row("esm.ExpDerived(a)", () => new esm.ExpDerived("a"));
row("esm.ExpNoCtorDerived(a)", () => new esm.ExpNoCtorDerived("a"));
row("esm.ExpNoCtorDerived(a,b,c)", () => new (esm.ExpNoCtorDerived as any)("a", "b", "c"));
row("esm.ExpNoCtorRest(a,b,c)", () => new esm.ExpNoCtorRest("a", "b", "c"));
const ENCD: any = esm.ExpNoCtorDerived;
row("value ExpNoCtorDerived(a)", () => new ENCD("a"));
const Capturing: any = esm.makeCapturing();
row("Capturing(a,b)", () => new Capturing("a", "b"));
const EV: any = esm.ExpTwo;
row("EV(a)", () => new EV("a"));

console.log("-- CommonJS classes --");
row("cjs.Request(url)", () => new cjs.Request("http://127.0.0.1:1/"));
row("cjs.Request(url,init)", () => new cjs.Request("http://127.0.0.1:1/", { method: "POST" }));
row("cjs.Request()", () => new cjs.Request());
row("cjs.Headers()", () => new cjs.Headers());
row("cjs.Headers(a,b)", () => new cjs.Headers("a", "b"));
row("cjs.StrictRequest(url)", () => new cjs.StrictRequest("http://x/"));
row("cjs.StrictRequest()", () => new cjs.StrictRequest());
const applier = new cjs.Applier("p", "q");
console.log("cjs.Applier(p,q) =>", applier.argc, JSON.stringify(applier.applied));
const { Request: DestructuredRequest } = cjs;
row("DestructuredRequest(url)", () => new DestructuredRequest("http://d/"));

// webidl shape declared in TypeScript.
function argumentLengthCheck({ length }: { length: number }, min: number, ctx: string) {
  if (length < min) throw new TypeError(`${ctx}: ${min} argument required, but only ${length} found.`);
}
class WebRequest {
  url: any;
  constructor(input: any, init: any = {}) {
    argumentLengthCheck(arguments, 1, "Request constructor");
    this.url = input;
  }
}
row("WebRequest(url)", () => new WebRequest("http://w/"));
row("WebRequest() via value", () => new (WebRequest as any)());

console.log("-- function constructor controls --");
function FnCtor(this: any, p?: any, q?: any) {
  this.r = fmtArgs(arguments);
}
const FV: any = FnCtor;
row("FnCtor(a)", () => new (FnCtor as any)("a"));
row("FV()", () => new FV());
row("FV(a,b,c)", () => new FV("a", "b", "c"));
row("Reflect.construct(FnCtor,[1])", () => Reflect.construct(FnCtor as any, ["x"]));

console.log("-- hot loop --");
let total = 0;
for (let i = 0; i < 1000; i++) {
  total += new LengthOnly(i).n + new V(i, i).r.length + (i % 2 ? new VL() : new VL(i, i, i)).n;
}
console.log("total", total);
