import { Base, Other, Reassigned, replaceParent } from './fixtures/awb2-static-proto/base.ts';

// Definition-time and later changes to an imported parent's prototype.
Base.prototype.bump = function () { return 11; };
class Child extends Base {
  step() { return this.value + 2; }
  run() { return this.step() + this.bump(); }
}
const child = new Child();
console.log('before', child.run());
Base.prototype.bump = function () { return 17; };
console.log('after', child.run());
Child.prototype.step = function () { return 23; };
console.log('override', child.run());
Object.defineProperty(Child.prototype, 'step', { configurable: true, get() { return () => 29; } });
console.log('accessor', child.run());
Object.defineProperty(Child.prototype, 'step', { configurable: true, writable: true, value() { return 31; } });
console.log('restored', child.run());
(child as any).step = () => 37;
console.log('own', child.run());
delete (child as any).step;
console.log('deleted-own', child.run());

Object.setPrototypeOf(Base.prototype, { ancestor() { return 41; } });
class Relinked extends Base { run() { return (this as any).ancestor(); } }
console.log('parent-prototype-link', new Relinked().run());
Object.setPrototypeOf(Base, Other);
console.log('parent-constructor-link', Object.getPrototypeOf(Base) === Other);
Object.setPrototypeOf(Child.prototype, { bump() { return 43; } });
console.log('child-link', child.run());

replaceParent();
class Changed extends Reassigned { run() { return this.bump(); } }
console.log('export-reassigned', new Changed().run());
function mix(parent: any) { return class extends parent { mixed() { return 47; } }; }
class Mixed extends mix(Base) { run() { return this.mixed() + this.bump(); } }
console.log('mixin', new Mixed().run());
class NullChild extends null { own() { return 53; } }
console.log('null', Object.getPrototypeOf(NullChild.prototype) === null, NullChild.prototype.own());
