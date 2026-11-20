// A dynamic site must use the current closure and current prototype.
function F(this: any, value: number) { this.a = value; }
function G(this: any, value: number) { this.b = value + 1; }
const holder: any = { ctor: F };
function make(value: number) { return new holder.ctor(value); }
console.log(make(1).a, make(2).a);
const before = make(3);
const original = F.prototype;
F.prototype = { marker: 7 };
const after = make(4);
console.log(before.a, after.a, after.marker);
console.log(Object.getPrototypeOf(before) === original, Object.getPrototypeOf(after) === F.prototype);
Object.defineProperty(F.prototype, 'marker', { get() { return 9; }, configurable: true });
console.log(make(5).marker);
holder.ctor = G;
console.log(make(6).b);
holder.ctor = F.bind(null, 10);
console.log(make(7).a);
holder.ctor = function(this: any, value: number) { this.a = value; return { result: value + 2 }; };
console.log(make(8).result);
holder.ctor = function(this: any, value: number) { this.a = value; return 42; };
console.log(make(9).a);
holder.ctor.prototype = 17;
console.log(make(10).a);
for (const ctor of [() => 1, undefined, 3]) {
    holder.ctor = ctor;
    try { make(11); console.log('bad constructor'); }
    catch (e) { console.log(e instanceof TypeError); }
}
function closureFactory(value: number) {
    return function(this: any) { this.a = value; };
}
for (let i = 0; i < 3; i++) {
    holder.ctor = closureFactory(i);
    console.log(make(0).a);
}
