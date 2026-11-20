// Uint8Array/Buffer copyWithin through the method site's byte-view entry
// (js_bm_copyWithin1..3) and its declines to the full dispatcher.
function show(label: string, v: Uint8Array): void {
  console.log(label, Array.from(v).join(","));
}
function cw1(u: Uint8Array, t: number): Uint8Array {
  return u.copyWithin(t);
}
function cw2(u: Uint8Array, t: number, s: number): Uint8Array {
  return u.copyWithin(t, s);
}
function cw3(u: Uint8Array, t: number, s: number, e: any): Uint8Array {
  return u.copyWithin(t, s, e);
}

show("one", cw1(new Uint8Array([1, 2, 3, 4, 5]), 2));
show("two", cw2(new Uint8Array([1, 2, 3, 4, 5]), 0, 3));
show("three", cw3(new Uint8Array([1, 2, 3, 4, 5]), 1, 3, 4));
show("undefinedEnd", cw3(new Uint8Array([1, 2, 3, 4, 5]), 0, 3, undefined));
show("negative", cw3(new Uint8Array([1, 2, 3, 4, 5]), -2, -4, -1));
show("overlapForward", cw3(new Uint8Array([1, 2, 3, 4, 5, 6]), 2, 0, 4));
show("overlapBackward", cw3(new Uint8Array([1, 2, 3, 4, 5, 6]), 0, 2, 6));
show("nonFinite", cw3(new Uint8Array([1, 2, 3, 4, 5]), NaN, 3, Infinity));
show("emptyRange", cw3(new Uint8Array([1, 2, 3, 4, 5]), 0, 4, 2));

const a = new Uint8Array([1, 2, 3, 4, 5, 6, 7, 8]);
console.log("returnsReceiver", cw3(a, 0, 4, 8) === a);
show("receiver", a);

const startObject = {
  valueOf() {
    console.log("valueOf ran");
    return 3;
  },
};
show("valueOf", cw3(new Uint8Array([1, 2, 3, 4, 5]), 0, startObject as any, undefined));

const view = new Uint8Array([9, 8, 7, 6, 5, 4]).subarray(1, 5);
show("subarray", cw3(view, 0, 2, 4));
show("buffer", cw3(Buffer.from([1, 2, 3, 4, 5]), 0, 3, 5));

const own = new Uint8Array([1, 2, 3]) as any;
own.copyWithin = function (this: any, t: number) {
  console.log("own method", t, this === own);
  return this;
};
cw3(own, 7, 0, 1);

const plain = {
  copyWithin(t: number, s: number, e: number) {
    console.log("plain object", t, s, e);
    return 1;
  },
};
cw3(plain as any, 1, 2, 3);
console.log("array", ([1, 2, 3, 4, 5] as any).copyWithin(0, 3, 5).join(","));
console.log("int16", Array.from((new Int16Array([1, 2, 3, 4, 5]) as any).copyWithin(0, 3, 5)).join(","));

// emscripten's _emscripten_memcpy_big shape: one large heap, many small copies.
const HEAPU8 = new Uint8Array(1 << 16);
for (let i = 0; i < HEAPU8.length; i++) HEAPU8[i] = i & 255;
function memcpyBig(dest: number, src: number, num: number): void {
  HEAPU8.copyWithin(dest, src, src + num);
}
let acc = 0;
for (let i = 0; i < 2000; i++) {
  const src = (i * 37) & 4095;
  const dest = 32768 + ((i * 101) & 8191);
  memcpyBig(dest, src, 512 + (i & 63));
  acc = (acc + HEAPU8[dest + (i % 512)]) | 0;
}
console.log("heap", acc);
