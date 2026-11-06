// Warm the same typed sites, then change the shape authority they consume.
class Meter {
  _value = 3;
  get amount() { return this._value * 3 + 7; }
  set amount(value: number) { this._value = value & 255; }
  get self() { return this; }
}
function read(m: Meter) { return m.amount; }
function write(m: Meter, n: number) { m.amount = n; return m._value; }
const m = new Meter();
for (let i = 0; i < 1000; i++) { write(m, i); read(m); }
console.log('initial', read(m), write(m, 12), m.self === m);
Object.defineProperty(Meter.prototype, 'amount', {
  configurable: true,
  get() { return this._value + 1000; },
  set(n: number) { this._value = n + 100; }
});
console.log('prototype', read(m), write(m, 2), read(m));
Object.defineProperty(m, 'amount', {
  configurable: true,
  get() { return this._value + 2000; },
  set(n: number) { this._value = n + 200; }
});
console.log('instance', read(m), write(m, 4), read(m));
class Child extends Meter {
  get amount() { return this._value + 3000; }
  set amount(n: number) { this._value = n + 300; }
}
const child = new Child();
console.log('override', read(child), write(child, 6), read(child));
class Parent {
  _value = 9;
  get answer() { return this._value; }
  set answer(n: number) { this._value = n; }
}
class Inherited extends Parent {}
function inherited(m: Inherited) { const v = m.answer; m.answer = v + 2; return m.answer; }
const inheritedMeter = new Inherited();
console.log('inherited', inherited(inheritedMeter));
Object.defineProperty(Inherited.prototype, 'answer', { configurable: true, get() { return 42; }, set(n: number) { this._value = n + 20; } });
console.log('intermediate', inherited(inheritedMeter));
// Negative control: a lying declared type cannot select the compiled body.
const unrelated: any = { _value: 70, get amount() { return this._value + 4000; }, set amount(n) { this._value = n + 400; } };
console.log('unrelated', read(unrelated), write(unrelated, 8), read(unrelated));
console.log('primitive', read(17 as any));
