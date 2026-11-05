// #10056: shared storage, offset/identity, empty views and independent copies.
function check(value: boolean, message: string): void {
  if (!value) throw new Error(message);
}

function inspectViews(source: any): void {
  for (let i = 0; i < source.length; i++) source[i] = i + 1;
  const left = source.subarray(2, 10);
  const right = source.subarray(4);
  const nested = left.subarray(2, 6);
  check(nested.buffer === source.buffer, "nested backing identity");
  check(nested.byteOffset === source.byteOffset + 4, "nested offset");
  check(nested.byteLength === 4, "nested byte length");
  source[4] = 51;
  check(nested[0] === 51 && right[0] === 51, "source writes");
  nested[1] = 61;
  check(source[5] === 61 && left[3] === 61 && right[1] === 61, "view writes");
  right.set(left.subarray(0, 4));
  check(source[4] === 3 && source[5] === 4 && source[6] === 51 && source[7] === 61,
    "overlapping set snapshots source");
  const tail = source.subarray(-3);
  check(tail.length === 3 && tail[0] === source[source.length - 3], "negative start");
  const all = source.subarray(-100, 100);
  check(all.length === source.length, "clamped bounds");
  const empty = source.subarray(6, 2);
  check(empty.length === 0 && empty.byteLength === 0, "empty view");
  check(empty.byteOffset === source.byteOffset + 6, "empty offset");
  check(empty.buffer === source.buffer, "empty backing");
  const edge = source.subarray(100);
  check(edge.length === 0 && edge.byteOffset === source.byteOffset + source.length, "clamped empty offset");
  check(source.subarray(2, undefined).length === source.length - 2, "undefined end");
  console.log("shared", source.length, left.length, right.length, nested.length, source[7]);
}

inspectViews(new Uint8Array(12));
inspectViews(Buffer.alloc(12));

const original = new Uint8Array([1, 2, 3, 4]);
const copy = original.slice(1, 3);
const view = original.subarray(1, 3);
original[1] = 20;
copy[1] = 30;
check(copy[0] === 2 && original[2] === 3 && view[0] === 20, "Uint8Array.slice copies");
check(copy instanceof Uint8Array && view instanceof Uint8Array, "view brands");
const borrowed = Uint8Array.prototype.slice.call(original, 1, 3);
original[1] = 21;
check(borrowed[0] === 20, "borrowed slice copies");

const buf = Buffer.from([1, 2, 3, 4, 5, 6, 7, 8]);
buf.subarray(0, 6).copy(buf.subarray(2), 0, 0, 6);
check(buf[2] === 1 && buf[7] === 6, "overlapping Buffer.copy");
const bufSlice = buf.slice(1, 3);
bufSlice[0] = 41;
check(buf[1] === 41, "Buffer.slice shares");

const ab = new ArrayBuffer(8);
const bytes = new Uint8Array(ab);
const dv = new DataView(ab, 2, 4);
const bytesView = bytes.subarray(2, 6);
dv.setUint16(0, 0x1234, true);
check(bytesView[0] === 0x34 && bytesView[1] === 0x12, "DataView writes");
bytesView[2] = 99;
check(dv.getUint8(2) === 99, "DataView reads");
const abCopy = ab.slice(2, 6);
bytesView[0] = 77;
check(new Uint8Array(abCopy)[0] === 0x34, "ArrayBuffer.slice copies");
console.log("copy and native views ok");

const consumerBuffer = Buffer.from([10, 20, 30, 40]).subarray(1, 3);
const consumerU8 = new Uint8Array([10, 20, 30, 40]).subarray(1, 3);
check(JSON.stringify(consumerBuffer) === '{"type":"Buffer","data":[20,30]}', "Buffer JSON");
check(JSON.stringify(consumerU8) === '{"0":20,"1":30}', "Uint8Array JSON");
console.log(JSON.stringify(consumerBuffer, null, 2));
console.log(JSON.stringify(consumerU8, null, 2));
console.log(consumerBuffer, consumerU8);
console.log(Array.from(consumerBuffer), Array.from(consumerU8));
