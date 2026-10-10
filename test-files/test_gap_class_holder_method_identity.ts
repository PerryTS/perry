// A declared method is one function object, whichever path reads it:
// prototype reflection, instance reads, super, private and static private
// methods, accessors and inheritance (#12300: method values are kept on the
// class holder).

class Base {
  greet(n: number) { return "base " + n; }
  get label() { return "B"; }
  set label(v: string) { this._l = v; }
  _l = "";
  static make() { return new this(); }
  #secret() { return "s" + this._l; }
  static #count() { return 7; }
  peek() { return this.#secret(); }
  peekRef() { return this.#secret; }
  static countRef() { return Base.#count; }
  static count() { return Base.#count(); }
}

class Derived extends Base {
  greet(n: number) { return "derived>" + super.greet(n); }
  superRef() { return super.greet; }
  get label() { return "D>" + super.label; }
}

const b = new Base();
const d = new Derived();
console.log(Base.prototype.greet === Base.prototype.greet);
console.log(b.greet === Base.prototype.greet, d.greet === Derived.prototype.greet);
console.log(d.superRef() === Base.prototype.greet);
console.log(b.peekRef() === b.peekRef(), b.peekRef() === d.peekRef());
console.log(Base.countRef() === Base.countRef(), Base.count());
console.log(d.greet(2), d.label, b.label);
d.label = "x";
console.log(d.peek());
console.log(Derived.make() instanceof Derived, Base.make() instanceof Derived);

const desc = Object.getOwnPropertyDescriptor(Base.prototype, "label")!;
console.log(typeof desc.get, typeof desc.set, desc.get === Object.getOwnPropertyDescriptor(Base.prototype, "label")!.get);
console.log(Object.getOwnPropertyNames(Base.prototype).join(","));
console.log(Object.getOwnPropertyNames(Derived.prototype).join(","));

// A method value read before and after the prototype is reflected.
class Late {
  run() { return 1; }
}
const early = new Late().run;
console.log(early === Late.prototype.run, early === new Late().run);

// Replacing and deleting a prototype method does not resurrect it.
class Mut {
  m() { return "orig"; }
}
const orig = Mut.prototype.m;
(Mut.prototype as any).m = function () { return "patched"; };
console.log(new Mut().m(), orig.call(null));
delete (Mut.prototype as any).m;
console.log(typeof (new Mut() as any).m);

// Class expressions evaluated many times: each evaluation's methods are its
// own, and identity holds within one evaluation.
function mk(tag: string) {
  return class {
    hello() { return tag; }
    #p() { return "p" + tag; }
    pref() { return this.#p; }
    call() { return this.#p(); }
  };
}
const objs = [];
for (let i = 0; i < 100; i++) {
  const K = mk("t" + i);
  const x = new K();
  objs.push([K, x] as const);
}
let same = 0;
let own = 0;
for (const [K, x] of objs) {
  if (x.hello === K.prototype.hello && x.pref() === x.pref()) same++;
  if (x.call() === "p" + x.hello()) own++;
}
console.log("eval-identity", same, own);
console.log(objs[0][0].prototype.hello !== objs[1][0].prototype.hello);
