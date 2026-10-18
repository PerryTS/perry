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
