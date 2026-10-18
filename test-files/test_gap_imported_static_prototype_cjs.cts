const { Base } = require('./fixtures/awb2-static-proto/base.cjs');
class Child extends Base {
  step() { return this.value + 2; }
  run() { return this.step() + this.bump(); }
}
const child = new Child();
console.log('canonical', child.run());
Base.prototype.bump = function () { return 11; };
console.log('parent-method', child.run());
Child.prototype.step = function () { return 17; };
console.log('child-method', child.run());
child.step = () => 23;
console.log('own-method', child.run());
delete child.step;
console.log('delete-own', child.run());
Object.setPrototypeOf(Child.prototype, { bump() { return 29; } });
console.log('prototype-link', child.run());

// A genuinely repeated nested definition must retain its own prototypes.
function make() {
  class Inner extends Base {
    constructor() { super(); this.local = 59; }
    run() { return this.local + this.bump(); }
  }
  return Inner;
}
const A = make();
const B = make();
console.log('fresh', A !== B, A.prototype !== B.prototype, new A().run(), new B().run());
A.prototype.run = function () { return 61; };
console.log('fresh-override', new A().run(), new B().run());
