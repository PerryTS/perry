// #3909: Buffer keys()/values()/entries() iterators must materialize via
// spread and Array.from (not just .next()/for-of), matching Node.
const buf = Buffer.from([10, 20, 30]);
console.log("spread keys", JSON.stringify([...buf.keys()]));
console.log("spread values", JSON.stringify([...buf.values()]));
console.log("spread entries", JSON.stringify([...buf.entries()]));
console.log("from keys", JSON.stringify(Array.from(buf.keys())));
console.log("from values", JSON.stringify(Array.from(buf.values())));
console.log("from entries len", Array.from(buf.entries()).length);
// Keep direct .next(), self-iterability, and for-of covered so iterator
// protocol consumers take the same path as Node.
const it = buf.values();
console.log("next", JSON.stringify(it.next()), JSON.stringify(it.next()));
const valuesIt = buf.values();
const keysIt = buf.keys();
const entriesIt = buf.entries();
valuesIt.next();
keysIt.next();
entriesIt.next();
console.log("values self next", JSON.stringify(valuesIt[Symbol.iterator]().next()));
console.log("keys self next", JSON.stringify(keysIt[Symbol.iterator]().next()));
console.log("entries self next", JSON.stringify(entriesIt[Symbol.iterator]().next()));
let sum = 0;
for (const v of buf.values()) sum += v;
console.log("for-of sum", sum);
let keySum = 0;
for (const k of buf.keys()) keySum += k;
console.log("for-of keys sum", keySum);
let entrySum = 0;
for (const [k, v] of buf.entries()) entrySum += k + v;
console.log("for-of entries sum", entrySum);
