// #6530: forward class references from methods of a CAPTURE-CARRYING class
// (classes nested in a function whose methods reference outer locals — the
// shape of Next.js's bundled zod, where ZodType captures its helper
// namespaces). Two paths regressed:
//   1. `Sub.create(this, ...)` from a sibling method — the post-class arrow
//      static lives on the per-evaluation class object, but the compiled
//      method dispatches through the INT32 ClassRef static table; the miss
//      silently returned the class ref itself.
//   2. `new Eff({...})` from a sibling method — the dynamic-parent value
//      super leg constructed-and-dropped a fresh parent instead of running
//      the parent constructor on `this`, so base-ctor fields stayed
//      undefined.

function makeModule() {
  function getParsedType(x: any): string {
    return typeof x;
  }
  const OK = (v: any) => ({ ok: true, value: v });

  class Base {
    _def: any;
    parse(x: any) {
      const ctx = { errorMap: this._def.errorMap, data: x, parsedType: getParsedType(x) };
      return this._parse(ctx);
    }
    _parse(ctx: any): any {
      return "base:" + ctx.data;
    }
    constructor(def: any) {
      this._def = def;
      this.parse = this.parse.bind(this);
      this.optional = this.optional.bind(this);
    }
    optional() {
      return (Sub as any).create(this, this._def);
    }
    transform(f: any) {
      return new Eff({ schema: this, effect: f, errorMap: "tm" });
    }
    describe(d: string) {
      const ctor: any = this.constructor;
      return new ctor({ ...this._def, description: d });
    }
  }

  class Eff extends Base {
    _parse(ctx: any) {
      return "eff:" + ctx.errorMap + ":" + ctx.data;
    }
  }

  class Sub extends Base {
    _parse(ctx: any) {
      const pt = getParsedType(ctx.data);
      if (pt === "undefined") {
        return OK(undefined).value;
      }
      return "sub:" + ctx.errorMap + ":" + ctx.data;
    }
    unwrap() {
      return this._def.innerType;
    }
  }
  (Sub as any).create = (e: any, t: any) => new Sub({ innerType: e, errorMap: "em" });

  return { Base, Sub, Eff };
}

const m = makeModule();

const b = new m.Base({ errorMap: "bm" });
console.log("base parse:", b.parse(1));

// path 1: post-class arrow static via forward ClassRef from a sibling method
const o = b.optional();
console.log("opt typeof:", typeof o);
console.log("opt instanceof Sub:", o instanceof m.Sub);
console.log("opt _def set:", o._def !== undefined);
console.log("opt parse undef:", o.parse(undefined));
console.log("opt parse val:", o.parse(7));
console.log("opt unwrap is base:", o.unwrap() === b);

// path 2: direct `new ForwardClass(...)` from a sibling method
const v = b.transform((x: any) => x);
console.log("eff instanceof Eff:", v instanceof m.Eff);
console.log("eff _def set:", v._def !== undefined);
console.log("eff parse:", v.parse(3));
console.log("eff schema is base:", v._def.schema === b);

// direct static call from outside (worked before, must keep working)
const s2 = (m.Sub as any).create(b, b._def);
console.log("direct _def set:", s2._def !== undefined);
console.log("direct parse:", s2.parse(9));
