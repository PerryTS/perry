function read(items: ReadonlyArray<any>, index: number) { return items[index]; }
console.log(read([10, 20], 1), read({ 1: 30 } as any, 1), read('ab' as any, 1));
console.log(read(new Uint8Array([40, 50]), 1));
const keys: any = [60, 70];
keys[0.5] = 80; keys[-1] = 90; keys.NaN = 100;
console.log(read(keys, 0.5), read(keys, -1), read(keys, NaN), read(keys, -0));
function replace(value: any) {
  let items: ReadonlyArray<any> = [1];
  const next = () => items[0];
  console.log(next());
  items = value;
  console.log(next());
}
replace({ 0: 110 });
replace('cd');
const inherited = new Array(1);
const proto = Object.create(Array.prototype);
Object.defineProperty(proto, '0', { get() { return 120; }, configurable: true });
Object.setPrototypeOf(inherited, proto);
console.log(read(inherited, 0));
