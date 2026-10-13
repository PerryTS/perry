class OrderBase { a = 3; b = 4; step(x: number): number { return this.b + x; } }
class OrderDerived extends OrderBase { step(x: number): number { return this.b * x; } }
const orderReceivers: OrderBase[] = [new OrderBase(), new OrderDerived()];
function ordered(c: OrderBase, change: () => number): number { const cs: OrderBase[] = [c]; return cs[0].step(change()); }
for (let i = 0; i < 20; i++) ordered(orderReceivers[i & 1], () => 2);
const orderProto: any = OrderBase.prototype;
console.log('selected', ordered(orderReceivers[0], () => { orderProto.step = function(x: number) { return 90 + x; }; return 2; }));
console.log('next', ordered(orderReceivers[0], () => 2));
const layout: any = orderReceivers[1];
console.log('layout', ordered(layout, () => { delete layout.a; return 2; }));
console.log('layout-next', ordered(layout, () => 2));
let lookups = 0;
Object.defineProperty(layout, 'step', { configurable: true, get() { lookups++; return function(x: number) { return 110 + x; }; } });
console.log('getter-before-arg', ordered(layout, () => { console.log('lookups-in-arg', lookups); return 2; }));
