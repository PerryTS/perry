// #11844: a class's prototype descriptor must expose its actual object,
// with the same identity and properties as an ordinary .prototype read.
class A {}
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(A, "prototype")));

function inspect(label: string, ctor: any): void {
  const descriptor = Object.getOwnPropertyDescriptor(ctor, "prototype")!;
  console.log(label, typeof descriptor.value, descriptor.value === ctor.prototype);
  console.log(descriptor.writable, descriptor.enumerable, descriptor.configurable);
  console.log(descriptor.value.constructor === ctor);
  console.log(Object.getOwnPropertyDescriptors(ctor).prototype.value === ctor.prototype);
  console.log(Reflect.getOwnPropertyDescriptor(ctor, "prototype")!.value === ctor.prototype);
  descriptor.value.marker = label;
  console.log(ctor.prototype.marker, new ctor().marker);
  console.log(JSON.stringify(Object.getOwnPropertyDescriptor(ctor, "prototype")));
  delete descriptor.value.marker;
}

inspect("declared", A);
class B extends A {}
inspect("derived", B);
const Expression = class {};
inspect("expression", Expression);
function makeClass(value: number): any {
  return class {
    getValue(): number { return value; }
  };
}
const First = makeClass(1);
const Second = makeClass(2);
inspect("first", First);
inspect("second", Second);
const firstPrototype = Object.getOwnPropertyDescriptor(First, "prototype")!.value;
const secondPrototype = Object.getOwnPropertyDescriptor(Second, "prototype")!.value;
console.log(firstPrototype !== secondPrototype);
console.log(firstPrototype.getValue(), secondPrototype.getValue());
