// Each evaluation owns its closures; the shared shape owns only body facts.
function make(value: number): any {
  let state = value;
  return {
    get: function () { return state; },
    set: function (v: number) { state = v; },
    has: function () { return state >= 0; },
    reset: function () { state = value; },
    twice: function () { return state * 2; }
  };
}
const a = make(7), b = make(11);
console.log("identity", a !== b, a.get !== b.get, a.set !== b.set, a.get(), b.get());
a.set(19);
console.log("captures", a.get(), b.get(), a.twice(), b.twice());
a.get = function () { return 101; };
console.log("reassigned", a.get(), b.get(), make(23).get());
a.has = 37;
console.log("value", a.has, b.has(), make(29).has());
function methods(value: number): any {
  return { value, get() { return this.value; }, add(n: number) { this.value += n; return this.value; },
    captured: () => value, fresh: function () { return value; } };
}
const m = methods(3), n = methods(5);
console.log("this", m.get(), n.get(), m.add(13), n.get(), m.captured(), n.fresh());
const detached = m.get;
console.log("rebind", detached.call(n), m.get.call({ value: 41 }), m.get !== n.get);
let order = "";
function next(v: number): number { order += v; return v; }
function ordered(): any {
  return { first: next(1), get: function () { return order; }, second: next(2),
    own: function () { return 31; } };
}
const o = ordered();
console.log("order", order, o.first, o.second, o.get(), o.own());

const host: any = {
  value: 47,
  make() {
    const captured = 53;
    return { lexical: () => this.value, captured: () => captured,
      method() { return this.value; }, value: 59 };
  }
};
const mixed = host.make();
console.log("lexical-this", mixed.lexical(), mixed.method(), mixed.captured());
