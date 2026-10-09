// Compile the instrumented arm with copying-minor stress and assert the
// collector's positive copied-object/protected-retired-set receipt separately.
let saved: any = { token: 'saved' };
let getter: any = () => 42;
const first: any = { value: saved, writable: true };
const access: any = { get: getter };
saved = null;
getter = null;
const symbol = Symbol('snapshot');
const properties: any = { saved_snapshot_value: first, saved_snapshot_accessor: access };
properties[symbol] = { value: { token: 'symbol' } };
Object.defineProperty(properties, 'late', { enumerable: true, get() {
  first.value = undefined;
  first.writable = false;
  access.get = undefined;
  for (let i = 0; i < 3000; i++) {
    const pressure = { i, text: 'snapshot-pressure-' + i };
    if (pressure.i < 0) throw new Error('unreachable');
  }
  return { value: 2 };
}});
const target: any = {};
Object.defineProperties(target, properties);
console.log('moving-snapshot', target.saved_snapshot_value.token, target.saved_snapshot_accessor,
  target[symbol].token, target.late, Object.getOwnPropertyDescriptor(target, 'saved_snapshot_value')!.writable);
const immutable: any = Object.defineProperty({}, 'x', { value: 1 });
let caught = 0;
for (let i = 0; i < 32; i++) {
  try {
    Object.defineProperty(new Proxy(immutable, { defineProperty(t, k, d) { d.value = 1; return true; } }), 'x', { value: 2 });
  } catch (error) { if (error instanceof TypeError) caught++; }
}
console.log('proxy-copy-recovery', caught, immutable.x);

// FromPropertyDescriptor creates own data fields without inherited setters.
const setterEvents: string[] = [];
const setterDescriptor: any = Object.create(null);
setterDescriptor.set = (value: any) => setterEvents.push(String(value));
setterDescriptor.configurable = true;
const userDescriptor: any = Object.create(null);
userDescriptor.value = 7;
userDescriptor.writable = false;
userDescriptor.configurable = true;
let copyFields = '';
Object.defineProperty(Object.prototype, 'value', setterDescriptor);
try {
  Reflect.defineProperty(new Proxy({}, { defineProperty(t, k, descriptor) {
    copyFields = String(Object.prototype.hasOwnProperty.call(descriptor, 'value')) + ':' +
      descriptor.value + ':' + descriptor.writable;
    return true;
  }}), 'x', userDescriptor);
} finally {
  delete (Object.prototype as any).value;
}
console.log('proxy-copy-own-fields', setterEvents.length, copyFields);
