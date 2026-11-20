// #11147: runtime-value super() preserves the actual construction target.
const ns: any = {
  Base: class Base {
    constructor(label: string) {
      (this as any).target = new.target;
      (this as any).label = label;
      console.log(label, typeof new.target, new.target === undefined ? 'undefined' : new.target.name);
    }
  },
};
class P extends ns.Base {}
const implicit: any = new P('implicit');
console.log('implicit-target', implicit.target === P);
class E extends ns.Base { constructor(label: string) { super(label); } }
new E('explicit');
const Alias = ns.Base;
class L extends Alias {}
new L('alias');
class S extends ns.Base { constructor(...args: any[]) { super(...args); } }
new S('spread');
class Deep extends E {}
const deep: any = new Deep('deep');
console.log('deep-target', deep.target === Deep);
function make(Base: any) {
  return class Local extends Base { constructor(label: string) { super(label); } };
}
const A: any = make(ns.Base), B: any = make(ns.Base);
console.log('fresh-a', new A('fresh').target === A);
console.log('fresh-b', new B('fresh').target === B);
class Alternate {}
const reflected: any = Reflect.construct(E, ['reflect'], Alternate);
console.log('reflect-target', reflected.target === Alternate, Object.getPrototypeOf(reflected) === Alternate.prototype);
const setup: any = {
  Base: class SetupBase {
    constructor() {
      return Object.assign(Object.create(new.target.prototype), { ready: 'ok' });
    }
  },
};
class Wrapper extends setup.Base { read() { return (this as any).ready; } }
console.log('wrapper', new Wrapper().read());
function plain() { return new.target; }
console.log('after-normal', plain() === undefined);
const fail: any = { Base: class ThrowBase { constructor() { throw new Error('expected'); } } };
class Fails extends fail.Base { constructor() { super(); } }
try { new Fails(); } catch (_) { console.log('after-throw', plain() === undefined); }
class Outer {
  constructor() {
    const inner: any = new E('nested');
    console.log('nested-target', inner.target === E, new.target === Outer);
    const implicit: any = new P('nested-implicit');
    const spread: any = new S('nested-spread');
    console.log('nested-others', implicit.target === P, spread.target === S, new.target === Outer);
  }
}
const constructors: any[] = [Outer];
new constructors[0]();
