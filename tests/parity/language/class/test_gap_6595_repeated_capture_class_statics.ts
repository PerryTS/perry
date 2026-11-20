// #6595 (regression of #6530 introduced by #6541): post-class arrow statics
// on REPEATED same-shaped capture-carrying classes. The receiver-based
// `[[Set]]` records a store plan for (template_cid, key) BEFORE the store
// reaches `js_object_set_field_by_name`, so from the SECOND same-shaped class
// object on (identical key-append sequence → shape-transition cache hit) the
// static write took the raw fast lane and skipped the #6530
// `CLASS_DYNAMIC_PROPS` mirror — sibling-method dispatch through the INT32
// ClassRef then missed and returned the class ref itself.
//
// Every class here must be CAPTURE-CARRYING (methods referencing outer
// locals) or it stays an INT32 ClassRef and the statics never touch the
// heap-class-object store path. One class alone can't catch the bug either:
// the first class of a shape misses the transition cache and falls to the
// mirroring path. Bundled zod has ~32 same-shaped classes; two suffice.

function makeModule() {
  const tag = (s: string) => "[" + s + "]";
  const isUndef = (x: any) => typeof x === "undefined";
  const isNull = (x: any) => x === null;

  class Base {
    _def: any;
    constructor(def: any) {
      this._def = def;
      this.parse = this.parse.bind(this);
      this.optional = this.optional.bind(this);
      this.nullable = this.nullable.bind(this);
    }
    parse(x: any) {
      return this._parse({ data: x, errorMap: this._def.errorMap });
    }
    _parse(ctx: any): any {
      return "base:" + ctx.data;
    }
    optional() {
      return (SubOpt as any).create(this, this._def);
    }
    nullable() {
      return (SubNull as any).create(this, this._def);
    }
    label() {
      return tag(this._def.name);
    }
  }

  class SubOpt extends Base {
    _parse(ctx: any) {
      if (isUndef(ctx.data)) return undefined;
      return "opt:" + ctx.errorMap + ":" + ctx.data;
    }
    unwrap() {
      return this._def.innerType;
    }
  }

  class SubNull extends Base {
    _parse(ctx: any) {
      if (isNull(ctx.data)) return null;
      return "null:" + ctx.errorMap + ":" + ctx.data;
    }
    unwrap() {
      return this._def.innerType;
    }
  }

  // Identical post-class static write sequence on same-shaped class objects:
  // the first write learns the shape-transition edge, the second must still
  // mirror into the ClassRef-world static table.
  (SubOpt as any).create = (inner: any, def: any) =>
    new SubOpt({ innerType: inner, errorMap: "eo", name: "opt" });
  (SubNull as any).create = (inner: any, def: any) =>
    new SubNull({ innerType: inner, errorMap: "en", name: "null" });

  return { Base, SubOpt, SubNull };
}

const m = makeModule();
const b = new m.Base({ errorMap: "bm", name: "base" });

// First same-shaped class: static via sibling-method ClassRef dispatch.
const o = b.optional();
console.log("opt typeof:", typeof o);
console.log("opt instanceof SubOpt:", o instanceof m.SubOpt);
console.log("opt _def set:", o._def !== undefined);
console.log("opt parse undef:", o.parse(undefined));
console.log("opt parse val:", o.parse(7));
console.log("opt label:", o.label());

// SECOND same-shaped class — the transition-cache-hit static write.
const n = b.nullable();
console.log("null typeof:", typeof n);
console.log("null instanceof SubNull:", n instanceof m.SubNull);
console.log("null _def set:", n._def !== undefined);
console.log("null parse null:", n.parse(null));
console.log("null parse val:", n.parse(3));
console.log("null label:", n.label());
console.log("null unwrap is base:", n.unwrap() === b);

// Chained: optional-of-nullable exercises both static dispatches in sequence.
const chained = b.nullable().optional();
console.log("chain typeof:", typeof chained);
console.log("chain parse undef:", chained.parse(undefined));
console.log("chain parse val:", chained.parse(5));
