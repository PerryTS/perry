// #10499: a class EXPRESSION's constructor `this.<name> = …` stores must be
// inferred as own fields exactly like the identical class DECLARATION's
// (perf: slot stores instead of a `[[Set]]` property add per store). This
// file pins the observable semantics that inference must not change.

// Plain base, the typescript.js `NodeObject` shape.
var NodeExpr: any = class {
  constructor(kind: any, pos: any, end: any) {
    this.pos = pos;
    this.end = end;
    this.kind = kind;
    this.id = 0;
    this.flags = 0;
    this.parent = undefined;
  }
};
const n = new NodeExpr(7, 1, 2);
console.log(Object.keys(n).join(","));
console.log(JSON.stringify(n));
n.pos = 10;
n.extra = "x";
console.log(n.pos, n.end, n.kind, n.extra, Object.keys(n).join(","));

// Insertion order follows execution order, including a chained assignment
// (`this.a = this.b = v` creates `b` first) and a minified comma sequence.
var Chain: any = class {
  constructor(v: any) {
    this.a = this.b = v;
    (this.c = 1), (this.d = 2);
  }
};
console.log(Object.keys(new Chain(3)).join(","));

// Self-binding an own method in the constructor is a method override, not a
// data field that shadows the method before the assignment runs.
var Bound: any = class {
  constructor() {
    this.seen = typeof this.run;
    this.run = this.run.bind(this);
  }
  run() {
    return this.seen;
  }
};
const bound = new Bound();
console.log(bound.run(), Object.keys(bound).join(","));

// An accessor written from the constructor goes through the setter.
var WithAccessor: any = class {
  constructor(p: any) {
    this._p = 0;
    this.points = p;
  }
  set points(v: any) {
    this._p = v * 2;
  }
  get points() {
    return this._p;
  }
};
const acc = new WithAccessor(4);
console.log(acc.points, Object.keys(acc).join(","));

// Class-expression subclass of a class-expression base: re-assigning a base
// field in the subclass must share the base's slot, not shadow it.
var Base: any = class {
  constructor() {
    this.kind = "base";
    this.shared = 1;
  }
  describe() {
    return this.kind + ":" + this.shared;
  }
};
var Sub: any = class extends Base {
  constructor() {
    super();
    this.kind = "sub";
    this.own = 2;
  }
};
const sub = new Sub();
console.log(sub.describe(), sub.own, Object.keys(sub).join(","));
console.log(sub instanceof Base, sub instanceof Sub);

// Class-DECLARATION subclass of a class-expression base.
class DeclSub extends NodeExpr {
  constructor() {
    super(1, 2, 3);
    this.pos = 99;
    this.tag = "decl";
  }
}
const ds: any = new DeclSub();
console.log(ds.pos, ds.end, ds.tag, Object.keys(ds).join(","));

// Class-expression subclass of a class declaration.
class DeclBase {
  constructor() {
    (this as any).x = 1;
  }
}
var ExprSub: any = class extends DeclBase {
  constructor() {
    super();
    this.x = 5;
    this.y = 6;
  }
};
const es = new ExprSub();
console.log(es.x, es.y, Object.keys(es).join(","));

// Parent chosen at runtime: the subclass must not assume the parent's layout.
var Left: any = class {
  constructor() {
    this.side = "left";
    this.l = 1;
  }
};
var Right: any = class {
  constructor() {
    this.side = "right";
    this.r = 2;
  }
};
function pick(left: boolean): any {
  return left ? Left : Right;
}
// Two distinct sites, each with its own runtime parent (#11042 covers one
// site re-evaluated over different parents).
const DynL: any = class extends pick(true) {
  constructor() {
    super();
    this.dyn = "d";
    this.side = this.side + "!";
  }
};
const DynR: any = class extends pick(false) {
  constructor() {
    super();
    this.dyn = "d";
    this.side = this.side + "!";
  }
};
for (const d of [new DynL(), new DynR()]) {
  console.log(d.side, d.l, d.r, d.dyn, Object.keys(d).join(","));
}

// Mixin: the parent is a parameter, unknown until the call.
function Tagged(BaseClass: any): any {
  return class extends BaseClass {
    constructor(...args: any[]) {
      super(...args);
      this.tagged = true;
      this.kind = "tagged-" + this.kind;
    }
  };
}
const TaggedNode = Tagged(NodeExpr);
const tn = new TaggedNode(4, 5, 6);
console.log(tn.kind, tn.pos, tn.tagged, Object.keys(tn).join(","));

// Class expression built inside a factory, capturing a local.
function makeClass(tag: string): any {
  return class {
    constructor(v: any) {
      this.value = v;
      this.tag = tag;
    }
    show() {
      return this.tag + "=" + this.value;
    }
  };
}
const A = makeClass("a");
const B = makeClass("b");
console.log(new A(1).show(), new B(2).show(), Object.keys(new A(3)).join(","));

// `module.exports = class …`-style anonymous expression in an object slot.
const exportsLike: any = {};
exportsLike.Res = class {
  constructor(points: any) {
    this.remainingPoints = points;
    this.consumedPoints = 0;
  }
};
const res = new exportsLike.Res(5);
console.log(res.remainingPoints, res.consumedPoints, Object.keys(res).join(","));

// Hot loop over many instances stays correct.
let s = 0;
const ring: any[] = new Array(64).fill(null);
for (let i = 0; i < 20000; i++) {
  const node = new NodeExpr(i & 63, -1, -1);
  node.pos = i;
  node.end = i + 5;
  ring[i & 63] = node;
  s = (s + node.kind + node.end) % 1000003;
}
console.log(s, ring[5].pos, ring[5].kind);
