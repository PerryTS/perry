const o: any = { a: 1, b: 2, get2() { return this.a + this.b; }, set(v: number) { this.a = v; return this.a; } };
console.log("basic", o.get2(), o.set(10), o.get2());
const other: any = { b: 5, a: 7, z: 1 };
console.log("call-other", o.get2.call(other), o.set.call(other, 3), other.a, other.z);
const derived: any = Object.create(o);
derived.a = 50;
console.log("proto", derived.get2(), o.a);
