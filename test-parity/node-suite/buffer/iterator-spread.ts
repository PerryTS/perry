// #3909: Buffer keys()/values()/entries() iterators must materialize via
// spread and Array.from (not just .next()/for-of), matching Node.
const buf = Buffer.from([10, 20, 30]);
console.log("spread keys", JSON.stringify([...buf.keys()]));
console.log("spread values", JSON.stringify([...buf.values()]));
console.log("spread entries", JSON.stringify([...buf.entries()]));
console.log("from keys", JSON.stringify(Array.from(buf.keys())));
console.log("from values", JSON.stringify(Array.from(buf.values())));
console.log("from entries len", Array.from(buf.entries()).length);
// .next() / for-of already worked — keep them covered so a regression in
// either direction is caught.
const it = buf.values();
console.log("next", JSON.stringify(it.next()), JSON.stringify(it.next()));
let sum = 0;
for (const v of buf.values()) sum += v;
console.log("for-of sum", sum);
