// #10487: a subclass constructor that assigns `this.m = ...` where `m` is a
// method inherited from a PARENT class hid the inherited method from the
// moment `super()` returned, instead of only once the assignment ran.
//
// `lower_class_decl` (crates/perry-hir/src/lower_decl/class_decl.rs) scans
// each constructor's `this.<name> = ...` assignments to decide whether
// `<name>` needs a synthesized inline field slot, excluding names that are
// declared fields, inherited fields, or accessors — and the class's OWN
// methods (the #665-adjacent zod `this.parse.bind(this)` fix: an own-method
// override must not get a shadowing data slot). It did not exclude METHODS
// INHERITED FROM THE EXTENDS CHAIN, so `this.close = ...` in a subclass
// constructor (where `close` is declared only on the parent) allocated an
// own `close` field. That field exists (as `undefined`) as soon as `super()`
// returns, so it shadows the inherited method in every by-name lookup until
// the assignment statement actually runs.

class Base {
  close() {
    return "closed";
  }
}
class Sub extends Base {
  seen: string;
  constructor() {
    super();
    this.seen = typeof this.close; // inherited method, read BEFORE the own assignment below
    this.close = () => "replaced";
  }
}
class Own {
  seen: string;
  close() {
    return "closed";
  }
  constructor() {
    this.seen = typeof this.close;
    this.close = () => "replaced";
  }
}
class Sub2 extends Base {
  seen: string;
  constructor() {
    super();
    this.seen = typeof this.close; // control: no own assignment to `close`
  }
}
class Sub3 extends Base {
  original: any;
  constructor() {
    super();
    this.original = this.close.bind(this); // undici MockPool / MockClient shape
    this.close = () => "replaced";
  }
}
// Grandparent-distance: the method is declared two levels up.
class Mid extends Base {}
class Grand extends Mid {
  seen: string;
  constructor() {
    super();
    this.seen = typeof this.close;
    this.close = () => "replaced";
  }
}
// Alias read: `const self = this; self.close` must see the same result.
class SubAlias extends Base {
  seen: string;
  constructor() {
    super();
    const self = this;
    this.seen = typeof self.close;
    this.close = () => "replaced";
  }
}
// Assignment made in a regular method (not the constructor): documented as
// already working, must keep working.
class SubMethodAssign extends Base {
  seen: string = "";
  replace() {
    this.seen = typeof this.close;
    this.close = () => "replaced";
  }
}

console.log("Sub      ", new Sub().seen);
console.log("Own      ", new Own().seen);
console.log("Sub2     ", new Sub2().seen);
console.log("Grand    ", new Grand().seen);
console.log("SubAlias ", new SubAlias().seen);
const sma = new SubMethodAssign();
sma.replace();
console.log("SubMethod", sma.seen);
try {
  console.log("Sub3", new Sub3().original());
} catch (e: any) {
  console.log("Sub3 threw", e.constructor.name, e.message);
}
