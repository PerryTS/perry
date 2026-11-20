// #10443: instance field initializers of a class whose DIRECT parent is a
// built-in Error constructor never ran when the class had its own constructor,
// so every field stayed `undefined` (mongodb's `MongoError.errorLabelSet`).
// Covers the whole Error family, with and without an explicit constructor,
// private `#fields`, static fields, and multi-level subclassing.

function show(label: string, value: unknown): void {
  console.log(label, value);
}

class WithCtor extends Error {
  n = 1;
  labels: Set<string> = new Set();
  list: string[] = [];
  #secret = 'p';
  static kind = 'WithCtor';
  constructor(message: string) {
    super(message);
    show('in ctor after super():', `${this.n} ${typeof this.labels}`);
  }
  get secret(): string {
    return this.#secret;
  }
}

const a = new WithCtor('m');
show('WithCtor n/labels/list:', `${a.n} ${a.labels instanceof Set} ${a.list.length}`);
show('WithCtor private:', a.secret);
show('WithCtor static:', WithCtor.kind);
show('WithCtor message/name:', `${a.message} ${a.name}`);
show('WithCtor instanceof:', `${a instanceof WithCtor} ${a instanceof Error}`);
show('WithCtor stack:', typeof a.stack === 'string' && a.stack.length > 0);
a.labels.add('x');
show('WithCtor labels after add:', a.labels.size);

class NoCtor extends Error {
  n = 2;
  labels: Set<string> = new Set();
}
const b = new NoCtor('m2');
show('NoCtor n/labels/message:', `${b.n} ${b.labels instanceof Set} ${b.message}`);

// Every native error base, with an explicit constructor.
class TE extends TypeError {
  n = 3;
  constructor(m: string) {
    super(m);
  }
}
class RE extends RangeError {
  n = 4;
  constructor(m: string) {
    super(m);
  }
}
class SE extends SyntaxError {
  n = 5;
  constructor(m: string) {
    super(m);
  }
}
class RfE extends ReferenceError {
  n = 6;
  constructor(m: string) {
    super(m);
  }
}
class EE extends EvalError {
  n = 7;
  constructor(m: string) {
    super(m);
  }
}
class UE extends URIError {
  n = 8;
  constructor(m: string) {
    super(m);
  }
}
show('TypeError:', `${new TE('t').n} ${new TE('t').message} ${new TE('t') instanceof TypeError}`);
show('RangeError:', `${new RE('r').n} ${new RE('r').message} ${new RE('r') instanceof RangeError}`);
show('SyntaxError:', `${new SE('s').n} ${new SE('s').message}`);
show('ReferenceError:', `${new RfE('rf').n} ${new RfE('rf').message}`);
show('EvalError:', `${new EE('e').n} ${new EE('e').message}`);
show('URIError:', `${new UE('u').n} ${new UE('u').message}`);

class AE extends AggregateError {
  n = 9;
  constructor(errors: Error[], m: string) {
    super(errors, m);
  }
}
const agg = new AE([new Error('inner')], 'agg');
// Only the field-initializer half is compared: perry maps `super(errors,
// message)` to `super(message)` for an AggregateError subclass, so its
// `message` / `errors` are a separate, pre-existing defect.
show('AggregateError:', `${agg.n} ${agg instanceof AggregateError} ${agg instanceof Error}`);

class DE extends DOMException {
  n = 18;
  constructor(m: string) {
    super(m, 'AbortError');
  }
}
const dom = new DE('dm');
show('DOMException:', `${dom.n} ${dom.message} ${dom.name} ${dom instanceof DOMException}`);

class DENoCtor extends DOMException {
  n = 19;
}
const dom2 = new DENoCtor('dm2', 'DataError');
show('DOMException no ctor:', `${dom2.n} ${dom2.message} ${dom2.name}`);

// Zero-argument constructor, statement before super(), class expression,
// class declared inside a function.
class ZeroArg extends Error {
  n = 10;
  constructor() {
    super('zero');
  }
}
show('zero-arg ctor:', `${new ZeroArg().n} ${new ZeroArg().message}`);

class BeforeSuper extends Error {
  n = 11;
  constructor(m: string) {
    const upper = m.toUpperCase();
    super(upper);
  }
}
show('stmt before super():', `${new BeforeSuper('bs').n} ${new BeforeSuper('bs').message}`);

const Expr = class extends Error {
  n = 12;
  constructor(m: string) {
    super(m);
  }
};
show('class expression:', `${new Expr('ce').n} ${new Expr('ce').message}`);

function makeLocal(): Error & { n: number } {
  class Local extends Error {
    n = 13;
    constructor(m: string) {
      super(m);
    }
  }
  return new Local('local');
}
const local = makeLocal();
show('class in function:', `${local.n} ${local.message}`);

// Multi-level: fields on every level, each level with its own constructor.
class MidA extends Error {
  m = 14;
  constructor(msg: string) {
    super(msg);
  }
}
class LeafB extends MidA {
  n = 15;
  constructor(msg: string) {
    super(msg);
  }
}
const leaf = new LeafB('leaf');
show('grandchild m/n/message:', `${leaf.m} ${leaf.n} ${leaf.message}`);
show('grandchild instanceof:', `${leaf instanceof LeafB} ${leaf instanceof MidA} ${leaf instanceof Error}`);

// A ctor-less level in the middle, and a ctor-less leaf.
class MidC extends Error {
  m = 16;
  constructor(msg: string) {
    super(msg);
  }
}
class LeafD extends MidC {
  n = 17;
}
const leafD = new LeafD('leafD');
show('ctor-less leaf m/n/message:', `${leafD.m} ${leafD.n} ${leafD.message}`);

// Field initializers must run exactly once — a side effect proves it.
let inits = 0;
class CountedBase extends Error {
  tick = ++inits;
  constructor(msg: string) {
    super(msg);
  }
}
class CountedLeaf extends CountedBase {
  own = ++inits;
  constructor(msg: string) {
    super(msg);
  }
}
const counted = new CountedLeaf('counted');
show('init order/count:', `${counted.tick} ${counted.own} ${inits}`);

// A private field installed twice would throw; this proves it is installed once.
class PrivBase extends Error {
  #tag = 'base';
  constructor(msg: string) {
    super(msg);
  }
  get tag(): string {
    return this.#tag;
  }
}
class PrivLeaf extends PrivBase {
  #own = 'leaf';
  constructor(msg: string) {
    super(msg);
  }
  get own(): string {
    return this.#own;
  }
}
const priv = new PrivLeaf('priv');
show('private chain:', `${priv.tag} ${priv.own} ${priv.message}`);

// The mongodb shape: a Set field plus a method that mutates it.
class MongoLikeError extends Error {
  private readonly errorLabelSet: Set<string> = new Set();
  constructor(message: string) {
    super(message);
  }
  addErrorLabel(label: string): void {
    this.errorLabelSet.add(label);
  }
  hasErrorLabel(label: string): boolean {
    return this.errorLabelSet.has(label);
  }
}
const mongo = new MongoLikeError('conn');
mongo.addErrorLabel('ResetPool');
show('mongo-like:', `${mongo.hasErrorLabel('ResetPool')} ${mongo.hasErrorLabel('Other')} ${mongo.message}`);

// Error subclass used through a catch clause keeps its fields.
try {
  throw new WithCtor('thrown');
} catch (e: unknown) {
  const err = e as WithCtor;
  show('caught:', `${err.n} ${err.message} ${err instanceof WithCtor}`);
}
