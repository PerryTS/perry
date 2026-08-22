// Constructor self-binding must override a method, not infer an uninitialized
// data field. This is the shape used by ZodType.
abstract class ZodType<O = any> {
  _def: any;
  spa = (this as any).safeParseAsync;
  constructor(def: any) {
    this._def = def;
    this.parse = this.parse.bind(this);
    this.safeParse = this.safeParse.bind(this);
    this.parseAsync = this.parseAsync.bind(this);
    this.safeParseAsync = this.safeParseAsync.bind(this);
    (this as any).spa = (this as any).spa.bind(this);
    this.optional = this.optional.bind(this);
    this.default = this.default.bind(this);
    this.catch = this.catch.bind(this);
  }
  abstract _parse(x: any): any;
  parse(d: unknown): O { return this._parse(d); }
  safeParse(d: unknown) { return { success: true, data: this._parse(d) }; }
  async parseAsync(d: unknown): Promise<O> { return this._parse(d); }
  async safeParseAsync(d: unknown) { return { success: true, data: this._parse(d) }; }
  optional() { return "optional"; }
  default(v: any) { return v; }
  catch(v: any) { return v; }
}
class ZodString extends ZodType<string> {
  _parse(x: any) { return "parsed:" + x; }
  static create = (): ZodString => new ZodString({ t: "s" });
}

const s: any = ZodString.create();
if (typeof s.parse !== "function") throw new Error("parse not a function");
if (s.parse("hi") !== "parsed:hi") throw new Error("parse() => " + s.parse("hi"));
if (s.optional() !== "optional") throw new Error("optional() => " + s.optional());
if (s.safeParse("x").data !== "parsed:x") throw new Error("safeParse data wrong");
console.log("OK");
