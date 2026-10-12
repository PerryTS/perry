let initial: any = 1;
class TypedChange { first = initial; second = 2; }
for (const value of [1, 'text', 3, null, 4, { n: 5 }, 6]) {
  initial = value;
  const object: any = new TypedChange();
  console.log(typeof object.first, object.second, Object.keys(object).join(','));
}
let snapshots = '';
function observe(value: any, label: string) {
  snapshots += label + ':' + Object.keys(value).join(',') + ';';
  return 1;
}
class Observed {
  first = observe(this, 'first');
  second = observe(this, 'second');
  third = observe(this, 'third');
}
for (let i = 0; i < 3; i++) { snapshots = ''; new Observed(); console.log(snapshots); }
// A mutation during an initializer changes the shape before DefineField.
let setterCalls = 0;
function install(value: any) {
  Object.defineProperty(value, 'second', { set(v) { setterCalls++; }, configurable: true });
  return 1;
}
class Mutation { first = install(this); second = 7; third = 8; }
for (let i = 0; i < 3; i++) {
  const value: any = new Mutation();
  const descriptor = Object.getOwnPropertyDescriptor(value, 'second')!;
  console.log(value.first, value.second, value.third, setterCalls, descriptor.writable, descriptor.enumerable, descriptor.configurable);
}
