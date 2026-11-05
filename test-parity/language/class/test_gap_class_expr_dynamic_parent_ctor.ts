// A derived class EXPRESSION with no own constructor must run the inherited
// constructor (implicit `constructor(...args) { super(...args) }`) — including
// when the parent is a runtime VALUE (dynamic parent edge), the shape bundlers
// emit for mysql2's promise mixin: `module.exports = class extends Pool {...}`.
import { EventEmitter } from 'events';

// [A] named base decl + named derived decl, no own ctor
class BaseA extends EventEmitter {
  config: any;
  constructor(o: any) { super(); this.config = o.config; }
}
class DerA extends BaseA { promise() { return 1; } }
const a = new DerA({ config: { x: 1 } });
console.log('[A]', typeof a.config, a.config && a.config.x);

// [B] anonymous base expr + anonymous derived expr, no own ctor
const BaseB: any = class extends EventEmitter {
  config: any;
  constructor(o: any) { super(); this.config = o.config; }
};
const DerB: any = class extends BaseB { promise() { return 1; } };
const b = new DerB({ config: { x: 2 } });
console.log('[B]', typeof b.config, b.config && b.config.x);

// [C] same but base extends a plain class
class P0 {}
const BaseC: any = class extends P0 {
  config: any;
  constructor(o: any) { super(); this.config = o.config; }
};
const DerC: any = class extends BaseC { promise() { return 1; } };
const c = new DerC({ config: { x: 3 } });
console.log('[C]', typeof c.config, c.config && c.config.x);

// [D] derived-no-ctor constructed via a type-erased variable
const mk = () => DerB;
const DerD = mk();
const d = new DerD({ config: { x: 4 } });
console.log('[D]', typeof d.config, d.config && d.config.x);

// [E] registry shape (turbopack factories): class values obtained via a
// lazily-run module table, mysql2 createPool chain end-to-end
const factories = new Map<number, any>();
const cache = new Map<number, any>();
const R = {
  r(id: number) {
    if (cache.has(id)) return cache.get(id).exports;
    const mod = { exports: {} as any };
    cache.set(id, mod);
    factories.get(id)!(R, mod, mod.exports);
    return mod.exports;
  },
};
factories.set(1, (E: any, _: any) => {
  _.exports = class {
    cc: any;
    constructor(o: any) { this.cc = { v: o.x }; }
  };
});
factories.set(2, (E: any, _: any) => {
  const t = EventEmitter;
  _.exports = class extends t {
    config: any;
    constructor(o: any) { super(); this.config = o.config; this.config.cc.pool = this; }
  };
});
factories.set(3, (E: any, _: any) => {
  const P = E.r(2);
  _.exports = class extends P { promise() { return 'promised'; } };
});
factories.set(4, (E: any, _: any) => {
  const Pool = E.r(3), Cfg = E.r(1);
  _.exports = function (o: any) { return new Pool({ config: new Cfg(o) }); };
});
const createPool = R.r(4);
const pool = createPool({ x: 42 });
console.log('[E]', typeof pool.config, pool.config.cc.v, typeof pool.config.cc.pool, pool.promise());

// [F] three-level chain: derived-no-ctor over derived-no-ctor over base
const Mid: any = class extends BaseB { };
const Leaf: any = class extends Mid { tag() { return 'leaf'; } };
const f = new Leaf({ config: { x: 6 } });
console.log('[F]', typeof f.config, f.config && f.config.x, f.tag());
