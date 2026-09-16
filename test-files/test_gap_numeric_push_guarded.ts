// The numeric push guard's fast arm calls a post-guard entry that re-derives
// only the receiver's iteration policy. Every receiver shape the guard does
// not admit — frozen, sealed, non-extensible, descriptor-bearing, a prototype
// index setter, a subclass, a Proxy, a Buffer/TypedArray — must still reach
// the same result as before, and a length that is not writable must not move.

function push(a: any, v: number): number {
  a.push(v);
  return a.length;
}

// Plain dense numeric array: the admitted shape.
const plain: number[] = [1, 2, 3];
console.log("plain", push(plain, 4), JSON.stringify(plain));
for (let i = 0; i < 40; i++) push(plain, i);
console.log("grown", plain.length, plain[43], JSON.stringify(plain.slice(0, 5)));

// Non-numeric values retire the dense claim through the same site.
const mixed: any[] = [1, 2];
push(mixed, 3);
mixed.push("s");
mixed.push({ o: 1 });
mixed.push(null);
console.log("mixed", mixed.length, JSON.stringify(mixed), typeof mixed[3]);

// Frozen / sealed / non-extensible receivers.
const frozen = Object.freeze([1, 2, 3]) as number[];
try {
  push(frozen, 4);
} catch (e: any) {
  console.log("frozen-throw", e.constructor.name);
}
console.log("frozen", frozen.length, JSON.stringify(frozen));

const sealed = Object.seal([1, 2, 3]) as number[];
try {
  push(sealed, 4);
} catch (e: any) {
  console.log("sealed-throw", e.constructor.name);
}
console.log("sealed", sealed.length, JSON.stringify(sealed));

// Non-extensible: only the resulting STATE is asserted. Perry does not throw
// here where node does (a pre-existing gap, identical before this change), and
// this fixture must not start failing on it.
const noExtend = Object.preventExtensions([1, 2, 3]) as number[];
try {
  push(noExtend, 4);
} catch {
  /* node throws, perry does not; both must leave the array untouched */
}
console.log("noextend", noExtend.length, JSON.stringify(noExtend));

// A non-writable length must refuse the push.
const fixedLen: number[] = [1, 2, 3];
Object.defineProperty(fixedLen, "length", { writable: false });
try {
  push(fixedLen, 4);
} catch (e: any) {
  console.log("fixedlen-throw", e.constructor.name);
}
console.log("fixedlen", fixedLen.length, JSON.stringify(fixedLen));

// An accessor defined on an index the push would write.
const accessor: any = [1, 2, 3];
let seen: any = null;
Object.defineProperty(accessor, 3, {
  set(v: any) {
    seen = v;
  },
  get() {
    return "acc";
  },
  configurable: true,
});
push(accessor, 99);
console.log("accessor", seen, accessor[3], accessor.length);

// Sparse receiver.
const sparse: any[] = [1, , 3];
push(sparse, 4);
console.log("sparse", sparse.length, 1 in sparse, JSON.stringify(sparse));

// Subclass and Proxy receivers.
class MyArr extends Array {}
const sub: any = MyArr.from([1, 2, 3]);
push(sub, 4);
console.log("subclass", sub.length, sub instanceof MyArr, JSON.stringify(Array.from(sub)));

const proxied: any = new Proxy([1, 2, 3], {
  set(t: any, k: any, v: any) {
    t[k] = v;
    return true;
  },
});
push(proxied, 4);
console.log("proxy", proxied.length, JSON.stringify(Array.from(proxied)));

// A mid-program Array.prototype index setter is observable on later pushes.
const beforeSetter: number[] = [1, 2];
push(beforeSetter, 3);
let protoSaw: any = null;
Object.defineProperty(Array.prototype, 7, {
  set(v: any) {
    protoSaw = v;
  },
  get() {
    return "proto";
  },
  configurable: true,
});
const afterSetter: number[] = [0, 1, 2, 3, 4, 5, 6];
push(afterSetter, 42);
console.log("proto-setter", protoSaw, afterSetter[7], afterSetter.length);
delete (Array.prototype as any)[7];

// Buffer / typed-array receivers route to their own paths.
const u8: any = new Uint8Array([1, 2, 3]);
console.log("u8-push-typeof", typeof u8.push);
const buf: any = Buffer.from([1, 2, 3]);
console.log("buffer-len", buf.length);
