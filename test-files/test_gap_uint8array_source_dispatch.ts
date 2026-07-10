// Gap test: Uint8Array constructor source dispatch must distinguish real
// Arrays from plain object/function array-likes and other iterable sources.
// Run: ./run_parity_tests.sh --filter uint8array_source_dispatch

function bytes(source: any): string {
  return Array.from(new Uint8Array(source)).join(",");
}

const plain = { 0: 5, 1: 6, length: 2 };
console.log("plain", bytes(plain));

function callableArrayLike(_a: unknown, _b: unknown) {}
(callableArrayLike as any)[0] = 8;
(callableArrayLike as any)[1] = 9;
console.log("function", bytes(callableArrayLike));

const iterable = {
  *[Symbol.iterator]() {
    yield 12;
    yield 13;
  },
  length: 99,
};
console.log("iterable", bytes(iterable));

const sparse = { 1: 21, length: 3 };
console.log("sparse", bytes(sparse));
console.log("array", bytes([31, 32]));
console.log("buffer", bytes(Buffer.from([41, 42])));

const ab = new ArrayBuffer(2);
new Uint8Array(ab).set([51, 52]);
console.log("arraybuffer", bytes(ab));
console.log("uint8", bytes(new Uint8Array([61, 62])));
console.log("int16", bytes(new Int16Array([257, -1])));

console.log("undefined", bytes(undefined));
console.log("null", bytes(null));
console.log("boolean", bytes(true));
console.log("string", bytes("2"));
try {
  bytes(Symbol("source"));
  console.log("symbol", "no throw");
} catch (error) {
  console.log("symbol", error instanceof TypeError);
}
try {
  bytes(1n);
  console.log("bigint", "no throw");
} catch (error) {
  console.log("bigint", error instanceof TypeError);
}
