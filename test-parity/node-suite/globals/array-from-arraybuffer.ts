// Array.from(source): a raw ArrayBuffer / SharedArrayBuffer is NOT array-like
// (no `length` property, no [[Symbol.iterator]]), so it takes the array-like
// branch with length 0 → an empty array, rather than materializing its bytes.
// Node Buffers and typed arrays ARE array-like / iterable and still materialize
// their elements. test262: built-ins/Array/from/items-is-arraybuffer.js
function show(label: string, value: unknown): void {
  console.log(label, JSON.stringify(value));
}

// Raw ArrayBuffer → empty array.
show("arraybuffer", Array.from(new ArrayBuffer(7) as unknown as ArrayLike<number>));
show("arraybuffer-empty", Array.from(new ArrayBuffer(0) as unknown as ArrayLike<number>));

// Node Buffer and typed arrays still materialize their elements.
show("buffer", Array.from(Buffer.from([1, 2, 3]) as unknown as ArrayLike<number>));
show("uint8", Array.from(new Uint8Array([4, 5, 6])));
show("uint16", Array.from(new Uint16Array([7, 8])));
show("uint8-mapped", Array.from(new Uint8Array([9, 10, 11]), (x) => x * 2));

// A typed-array view over an ArrayBuffer is array-like (length = element count).
const ab = new ArrayBuffer(4);
const view = new Uint8Array(ab);
view[0] = 1;
view[3] = 4;
show("view", Array.from(view));

// Other array-like / iterable sources are unchanged.
show("arraylike", Array.from({ length: 2, 0: "a", 1: "b" } as ArrayLike<string>));
show("set", Array.from(new Set([1, 2, 2, 3])));
show("string", Array.from("hi"));
show("array", Array.from([1, 2, 3]));
