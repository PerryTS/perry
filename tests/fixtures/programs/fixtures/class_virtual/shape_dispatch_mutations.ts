class A { value = 1; run() { return this.value + 10; } }
class B extends A { run() { return this.value + 20; } }
class C extends A { run() { return this.value + 30; } }
class D extends A { run() { return this.value + 40; } }
class E extends A { run() { return this.value + 50; } }
class F extends A { run() { return this.value + 60; } }
class G extends A { run() { return this.value + 70; } }
class H extends A { run() { return this.value + 80; } }
class I extends A { run() { return this.value + 90; } }
class Inherited extends A { extra = 9; }
class Super extends A { run() { return super.run() + 100; } }
function dynamic(x: any) { return x.run(); }
function narrow(xs: A[], i: number) { return xs[i].run(); }
const a: any = new A();
console.log('own', dynamic(a), narrow([a], 0));
a.run = function() { return this.value + 200; };
console.log('own-added', dynamic(a), narrow([a], 0));
delete a.run;
console.log('own-deleted', dynamic(a), narrow([a], 0));
const b: any = new B();
console.log('prototype', dynamic(b), narrow([b], 0));
B.prototype.run = function() { return this.value + 300; };
console.log('prototype-replaced', dynamic(b), narrow([b], 0));
let gets = 0;
Object.defineProperty(B.prototype, 'run', {
  configurable: true, get() { gets++; return function() { return this.value + 400; }; }
});
console.log('prototype-getter', dynamic(b), narrow([b], 0), gets);
Object.defineProperty(B.prototype, 'run', {
  configurable: true, writable: true, value: function() { return this.value + 500; }
});
console.log('prototype-defined', dynamic(b), narrow([b], 0));
const p: any = new A();
console.log('edge-before', dynamic(p));
// Explicit opt-in retains the independently observed main failure as evidence.
if (process.argv[2] === 'include-prototype-edge') {
  Object.setPrototypeOf(p, {run() { return this.value + 600; }});
  console.log('edge-after', dynamic(p), narrow([p], 0));
}
const receivers: A[] = [new A(), new C(), new D(), new E(), new F(), new G(), new H(), new I()];
let sum = 0;
for (let repeat = 0; repeat < 3; repeat++) {
  for (let i = 0; i < receivers.length; i++) sum += dynamic(receivers[i]) + narrow(receivers, i);
}
console.log('polymorphic', sum);
console.log('inherited-super', dynamic(new Inherited()), dynamic(new Super()));
const ownGetter: any = new C();
console.log('own-getter-before', dynamic(ownGetter));
Object.defineProperty(ownGetter, 'run', {get() { return () => 700; }});
console.log('own-getter-after', dynamic(ownGetter), narrow([ownGetter], 0));
let order = '';
const observable: any = new D();
Object.defineProperty(observable, 'run', {get() { order += 'get '; return () => 800; }});
function argument() { order += 'arg '; return 1; }
function ordered(x: any) { return x.run(argument()); }
console.log('order', ordered(observable), order);
const plain = {value: 4, run() { return this.value + 900; }};
console.log('negative', dynamic(plain));
