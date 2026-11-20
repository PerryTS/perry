// #12668: a class with no own constructor runs its nearest ancestor's body.
// Scalar replacement of a non-escaping `const o = new Sub()` inlines that
// inherited body, so the escape check has to look at the same body: a base
// constructor that materializes `this` (bind, arrow capture, method/getter
// call, passing `this` along) must keep the subclass instance on the heap.
// Before the fix `this` inside the inlined body was undefined and
// `this.parse.bind(this)` threw "Bind must be called on a function".

class BindBase {
  a1: number;
  constructor() {
    this.parse = this.parse.bind(this);
    this.a1 = 1;
  }
  parse(x: number) {
    return x;
  }
}
class BindSub extends BindBase {}
class BindLeaf extends BindSub {}

function bindSub(): number {
  let s = 0;
  for (let i = 0; i < 3; i++) {
    const o = new BindSub();
    s += o.a1;
  }
  return s;
}
function bindLeafUnused(): string {
  const o = new BindLeaf();
  return "ok";
}
function bindLeafRead(): number {
  const o = new BindLeaf();
  return o.a1;
}
console.log("bind sub", bindSub());
console.log("bind leaf unused", bindLeafUnused());
console.log("bind leaf read", bindLeafRead());

class MethodBindBase {
  k: number;
  constructor() {
    this.run = this.run.bind(this);
    this.k = 2;
  }
  run() {
    return this.k;
  }
}
class MethodBindSub extends MethodBindBase {}
function methodBind(): number {
  let s = 0;
  for (let i = 0; i < 4; i++) {
    const o = new MethodBindSub();
    s += o.k;
  }
  return s;
}
console.log("method bind", methodBind());

class ArrowBase {
  v: number;
  constructor(v: number) {
    this.v = v;
    const get = () => this.v * 10;
    this.v = get();
  }
}
class ArrowSub extends ArrowBase {}
function arrow(): number {
  const o = new ArrowSub(7);
  return o.v;
}
console.log("arrow", arrow());

class GetterBase {
  seen: number;
  constructor() {
    this.seen = this.twice;
  }
  get twice(): number {
    return 21 * 2;
  }
}
class GetterSub extends GetterBase {}
class GetterLeaf extends GetterSub {}
function getter(): number {
  const o = new GetterLeaf();
  return o.seen;
}
console.log("getter", getter());

const registry: any[] = [];
class RegBase {
  id: number;
  constructor(id: number) {
    this.id = id;
    registry.push(this);
  }
}
class RegSub extends RegBase {}
function reg(): number {
  const o = new RegSub(5);
  return o.id;
}
console.log("register", reg(), registry.length, registry[0] instanceof RegSub);

class MidFieldsBase {
  b: number;
  constructor() {
    this.parse = this.parse.bind(this);
    this.b = 3;
  }
  parse() {
    return this.b;
  }
}
class MidFields extends MidFieldsBase {
  extra = 4;
}
class MidFieldsLeaf extends MidFields {
  more = 5;
}
function midFields(): number {
  const o = new MidFieldsLeaf();
  return o.b + o.extra + o.more;
}
console.log("mid fields", midFields());

// The safe shape stays fine: a base ctor that only stores declared fields.
class PlainBase {
  x: number;
  y: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
}
class PlainSub extends PlainBase {}
function plain(): number {
  let s = 0;
  for (let i = 0; i < 5; i++) {
    const p = new PlainSub(i, 2 * i);
    s += p.x + p.y;
  }
  return s;
}
console.log("plain", plain());
