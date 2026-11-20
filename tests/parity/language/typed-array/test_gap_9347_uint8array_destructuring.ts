// Uint8Array uses Perry's buffer representation while Int32Array uses the
// typed-array representation.  Both must drive the iterator protocol during
// binding and assignment destructuring.
function makeValues<T extends Uint8Array | Int32Array>(value: T): T {
  value[0] = 6;
  value[1] = 7;
  value[2] = 8;
  return value;
}

function binding(label: string, value: Uint8Array | Int32Array): void {
  const [first, ...rest] = value;
  console.log(label, first, rest.join(","));
}

function assignment(label: string, value: Uint8Array | Int32Array): void {
  let first = 0;
  let rest: number[] = [];
  [first, ...rest] = value;
  console.log(label, first, rest.join(","));
}

const i32 = makeValues(new Int32Array(3));
const u8 = makeValues(new Uint8Array(3));

binding("binding-i32", i32);
binding("binding-u8", u8);
assignment("assignment-i32", i32);
assignment("assignment-u8", u8);
