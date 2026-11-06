const log: string[] = [];
const p = new Proxy([10, 20], { get(t, k, r) { log.push(String(k)); return Reflect.get(t, k, r); } });
console.log(JSON.stringify(Array.from(p)));
console.log(log.join(','));
const q = new Proxy({length: 2, 0: 'a', 1: 'b'}, {get(t, k, r) { return Reflect.get(t, k, r); }});
console.log(JSON.stringify(Array.from(q)));
const r = new Proxy({}, {get(t, k) { if (k === Symbol.iterator) return function* () { yield 7; yield 8; }; }});
console.log(JSON.stringify(Array.from(r)));

// Null @@iterator selects indexed reads, and mapping retains the proxy receiver.
const reads: string[] = [];
const arrayLike = new Proxy({ 0: 3, 1: 4, length: 2 }, {
  get(target, key, receiver) {
    reads.push(String(key));
    if (key === Symbol.iterator) return null;
    return Reflect.get(target, key, receiver);
  }
});
console.log(JSON.stringify(Array.from(arrayLike)));
console.log(reads.join(','));
console.log(JSON.stringify(Array.from(arrayLike, (value, index) => value + index)));
const revoked = Proxy.revocable([1], {});
revoked.revoke();
try { Array.from(revoked.proxy); } catch (error) { console.log(error instanceof TypeError); }
