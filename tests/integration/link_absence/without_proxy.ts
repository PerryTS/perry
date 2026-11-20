const target: any = {x: 2};
const receiver: any = {};
console.log(Reflect.set(target, "x", 7, receiver), target.x, receiver.x);
Object.defineProperty(target, "y", {set(v) { this.z = v + 1; }, configurable: true});
console.log(Reflect.set(target, "y", 9, receiver), receiver.z);
const child: any = Object.create(target); child.x = 12;
console.log(child.x, target.x, Reflect.get(child, "x"), Reflect.has(child, "y"));
console.log(Reflect.deleteProperty(child, "x"), child.x);
class C { value = 3; method() { return this.value; } }
const c: any = new C();
console.log(c.method(), typeof C, Reflect.construct(C, []).method());
console.log(Reflect.ownKeys(receiver).join(","), Reflect.isExtensible(receiver));
console.log(Reflect.preventExtensions(receiver), Reflect.isExtensible(receiver));
