function get(value: any, key: string) { return value[key]; }
// DEC 250: Array.prototype refuses a program relink, so the relinked link is a
// user array prototype between the array and Array.prototype.
const arrayProto: any = Object.create(Array.prototype);
const original = Object.getPrototypeOf(arrayProto);
const array: any = [];
Object.setPrototypeOf(array, arrayProto);
const parent: any = Object.create(null);
parent.marker = 41;
parent[0] = 'inherited index';
Object.defineProperty(parent, 'receiver', { get() { return this; }, configurable: true });
try {
  Object.setPrototypeOf(arrayProto, parent);
  console.log('prototype', Object.getPrototypeOf(arrayProto) === parent);
  console.log('named', get(arrayProto, 'marker'), get(array, 'marker'));
  console.log('removed parent', get(array, 'push') === undefined);
  console.log('accessor', get(array, 'receiver') === array);
  console.log('indexed', get(array, '0'), 0 in array);
  parent.marker = 42;
  console.log('changed', get(array, 'marker'));
  Object.setPrototypeOf(arrayProto, null);
  console.log('null', get(array, 'marker') === undefined, get(array, '0') === undefined);
} finally {
  Object.setPrototypeOf(arrayProto, original);
}
console.log('restored', Object.getPrototypeOf(arrayProto) === original);
