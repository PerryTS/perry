// parity-node-argv: --expose-gc
// DEC 250: WeakMap.prototype.get is locked in a compiled program, so the accessor redefinitions are refused and every read runs the builtin get.
// tests/parity/expected holds Perry's output.
function locked(f: () => void) { try { f(); } catch (e) { console.log("DEC 250: " + (e as Error).message); } }
export {};
declare function gc(): void;
const key = {};
const choices: any[] = [new WeakMap(), {}];
const map: any = choices[0];
map.set(key, 73);
function read(receiver: any) { return receiver.get(key); }
for (let i = 0; i < 100; i++) read(map);
const proto: any = WeakMap.prototype;
const original = proto.get;
let calls = 0;
locked(() => { Object.defineProperty(proto, "get", { configurable: true, get() {
  calls++;
  gc();
  return function(k: any) {
    return this === map && k === key ? original.call(this, k) : -1;
  };
}}); });
console.log(read(map), calls);
locked(() => { Object.defineProperty(proto, "get", { configurable: true, get() {
  calls++;
  return undefined;
}}); });
console.log(typeof map.get, calls);
locked(() => { Object.defineProperty(proto, "get", { value: original, writable: true, configurable: true }); });
console.log(read(map));
