// A Buffer is an ordinary Uint8Array in Node: you can hang own properties on it,
// read `Buffer.prototype` methods as VALUES, and shadow them with own properties.
// mysql2 relies on all three (its `MockBuffer` measures a packet before allocating
// it by replacing the write methods of a zero-length Buffer with no-ops):
//
//   for (const k in Buffer.prototype)
//     if (typeof mock[k] === "function") mock[k] = noop;
//
// Perry stored Buffer instances as a raw BufferHeader with no property table, so
// own props vanished on write and every method-VALUE read came back undefined.

const b: any = Buffer.alloc(8);

// own data properties
b.myFlag = 42;
b.label = "packet";
console.log("own number   :", b.myFlag, typeof b.myFlag);
console.log("own string   :", b.label);

// a method read as a VALUE (not called) — the `typeof mock[k] === "function"` probe
console.log("method value :", typeof b.readUInt8, typeof b.writeUInt8, typeof b.slice);
console.log("missing key  :", typeof b.notAMethod);

// `in` and key enumeration still see the prototype methods
console.log("in operator  :", "readUInt8" in b, "myFlag" in b);

// calling through a value read binds `this` to the buffer
const reader = b.readUInt8;
b[0] = 0xab;
console.log("value call   :", reader.call(b, 0));

// an own property shadows the prototype method on the dynamic dispatch path
// (a statically-provable buffer receiver with a literal method name still folds
// to the inline byte-load intrinsic, which cannot see own props — see #6405)
b.readUInt8 = function () {
  return "shadowed";
};
const key = "readUInt8";
console.log("dynamic key  :", b[key](0));

// numeric indices remain byte access, never own props
b[1] = 200;
console.log("byte index   :", b[0], b[1], b.length);

// a freshly allocated Buffer must not inherit a recycled address's own props
const c: any = Buffer.alloc(4);
console.log("fresh buffer :", c.myFlag, typeof c.readUInt8, c.readUInt8(0));

// own props survive a write through the buffer's own methods
const d: any = Buffer.alloc(4);
d.tag = "keep";
d.writeUInt8(7, 0);
console.log("after write  :", d.tag, d[0]);
