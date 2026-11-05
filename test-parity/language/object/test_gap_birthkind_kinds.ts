// Birth kinds must retain their own layout and [[Prototype]] semantics.
const proto: any = { inherited: 17 };
class Base { value = 23; read() { return this.value; } }
const plain: any = { value: 11 };
const nullish: any = Object.create(null);
nullish.value = 13;
const created: any = Object.create(proto);
created.value = 19;
const instance: any = new Base();
const array: any = [29, 31];
const callable: any = () => 37;
callable.value = 41;
for (const [name, object] of [
  ["plain", plain], ["null", nullish], ["created", created],
  ["instance", instance], ["array", array], ["callable", callable]
] as any[]) {
  Object.defineProperty(object, "added", { value: 43, writable: true, enumerable: true, configurable: true });
  const desc: any = Object.getOwnPropertyDescriptor(object, "added");
  console.log(name, typeof object, Array.isArray(object), object.added,
    desc.writable, desc.enumerable, desc.configurable);
}
console.log("birth", Object.getPrototypeOf(nullish) === null,
  Object.getPrototypeOf(created) === proto, created.inherited,
  instance instanceof Base, instance.read(), array.length, array.join("|"), callable());
const other: any = { inherited: 47 };
for (const object of [plain, nullish, created, instance, array, callable]) {
  Object.setPrototypeOf(object, other);
  Object.defineProperty(object, "computed", {
    get() { return this.added + this.inherited; },
    set(v) { this.added = v - this.inherited; }, enumerable: true, configurable: true
  });
  console.log("changed", Object.getPrototypeOf(object) === other, object.computed);
  object.computed = 101;
  console.log("stored", object.added, object.computed);
  delete object.computed;
  object.computed = 107;
  console.log("data", object.computed, Object.getOwnPropertyDescriptor(object, "computed")!.writable);
}
console.log("layouts", Array.isArray(array), array.length, array[0], callable());
const a: any = Object.create(proto), b: any = Object.create(other);
console.log("identities", Object.getPrototypeOf(a) === proto, Object.getPrototypeOf(b) === other,
  a.inherited, b.inherited);
