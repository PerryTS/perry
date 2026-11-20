// Completed numeric layouts retain definition order and revoke on mutation.
class Ordered {
  a: number;
  b: number;
  constructor(x: number) { this.b = x + 1; this.a = x; }
  sum(): number { return this.a + this.b; }
}
const ordered = new Ordered(3);
console.log(Object.keys(ordered).join(','), ordered.sum());
(ordered as any).a = 'changed';
console.log(ordered.a, ordered.b);
Object.defineProperty(ordered, 'b', { get() { return 17; }, configurable: true });
console.log(ordered.b);
delete (ordered as any).a;
(ordered as any).a = 9;
console.log(Object.keys(ordered).join(','), ordered.a, ordered.b);

// A value observable before its constructor store must still be undefined.
class Observed {
  a: number;
  b = [Object.keys(this).join(','), this.a];
  constructor() { this.a = 4; }
}
const observed = new Observed();
console.log(JSON.stringify(observed.b), observed.a);

class Parent {
  x: number;
  constructor(x: number) { this.x = x; }
  get scaled(): number { return this.x * 3; }
}
class Child extends Parent {
  y: number;
  constructor(x: number) { super(x); this.y = x + 1; }
}
const child = new Child(5);
console.log(Object.keys(child).join(','), child.scaled, child.y);
console.log((new Child('text' as any)).x);

function factory() {
  return class extends Parent {
    y: number;
    constructor(x: number) { super(x); this.y = x + 2; }
  };
}
const First = factory(), Second = factory();
const first = new First(2), second = new Second(3);
console.log(first.scaled, first.y, second.scaled, second.y, First === Second);

// Constructor reservations create only the keys whose stores execute.
class Conditional {
  constructor(add: boolean) {
    if (add) (this as any).b = 2;
    (this as any).a = 1;
  }
}
console.log(Object.keys(new Conditional(false)).join(','));
console.log(Object.keys(new Conditional(true)).join(','));

// An ordinary assignment to an absent key can observe defaults via a setter.
class SetterObserver {
  a: number;
  set signal(v: number) { console.log('setter-default', this.a, v); }
  constructor() { this.signal = 1; this.a = 2; }
}
console.log(new SetterObserver().a);
class DynamicSetterObserver {
  a: number;
  constructor() { (this as any).signal = 1; this.a = 2; }
}
Object.defineProperty(DynamicSetterObserver.prototype, 'signal', {
  set(v) { console.log('dynamic-setter-default', this.a, v); }
});
console.log(new DynamicSetterObserver().a);

// Numeric reservations stay absent while a constructor's setter can observe them.
class ReservationObserver {
  constructor() { (this as any).signal = 1; this.a = 2; this.b = 3; }
  get pending(): number { return this.b; }
}
Object.defineProperty(ReservationObserver.prototype, 'signal', {
  set() { console.log('reservation-default', this.b, this.pending); }
});
const reservation = new ReservationObserver();
console.log(Object.keys(reservation).join(','), reservation.a, reservation.pending);

// A nonnumeric definition must preserve numeric lanes inherited from the parent.
class MixedChild extends Parent {
  label: string = 'mixed';
  y: number;
  constructor(x: number) { super(x); this.y = x + 7; }
}
const mixed = new MixedChild(6);
console.log(Object.keys(mixed).join(','), mixed.scaled, mixed.label, mixed.y);
