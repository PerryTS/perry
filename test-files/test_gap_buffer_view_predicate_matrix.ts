// #11239: every typed-array / view predicate must agree that a Node Buffer
// (from any constructor path) is a Uint8Array view, matching Node.
import { types } from "node:util";

const isView = ArrayBuffer.isView;
const isABV = types.isArrayBufferView;
const isTA = types.isTypedArray;
const isU8 = types.isUint8Array;
const isAnyAB = types.isAnyArrayBuffer;
const isBuf = Buffer.isBuffer;

function row(label: string, v: any): void {
  const tag = Object.prototype.toString.call(v);
  let ctor = "-";
  try {
    ctor = v !== null && v !== undefined && v.constructor ? String(v.constructor.name) : "-";
  } catch (e) {
    ctor = "throw";
  }
  console.log(
    label.padEnd(14),
    [
      ArrayBuffer.isView(v),
      isView(v),
      types.isArrayBufferView(v),
      isABV(v),
      types.isTypedArray(v),
      isTA(v),
      types.isUint8Array(v),
      isU8(v),
      types.isInt8Array(v),
      types.isUint8ClampedArray(v),
      types.isDataView(v),
      types.isAnyArrayBuffer(v),
      isAnyAB(v),
      types.isArrayBuffer(v),
      v instanceof Uint8Array,
      v instanceof Buffer,
      Buffer.isBuffer(v),
      isBuf(v),
    ]
      .map((b) => (b ? "T" : "f"))
      .join(""),
    tag,
    ctor,
  );
}

const base = Buffer.alloc(8, 7);
const FastBuffer = (Buffer as any)[Symbol.species];

row("alloc", base);
row("alloc0", Buffer.alloc(0));
row("allocUnsafe", Buffer.allocUnsafe(4));
row("allocUnsafeSl", Buffer.allocUnsafeSlow(4));
row("from-str", Buffer.from("abc"));
row("from-hex", Buffer.from("00ff", "hex"));
row("from-arr", Buffer.from([1, 2, 3]));
row("from-buf", Buffer.from(base));
row("from-u8", Buffer.from(new Uint8Array([1, 2])));
row("from-ab", Buffer.from(new ArrayBuffer(32), 8, 16));
row("concat", Buffer.concat([base, Buffer.from("x")]));
row("slice", base.slice(1, 3));
row("subarray", base.subarray(1, 3));
row("sub-of-sub", base.subarray(1).subarray(1, 3));
row("fast-size", new FastBuffer(4));
row("fast-ab", new FastBuffer(new ArrayBuffer(16), 4, 8));
row("uint8", new Uint8Array(4));
row("u8-subarray", new Uint8Array(4).subarray(1));
row("int8", new Int8Array(4));
row("u8clamped", new Uint8ClampedArray(4));
row("uint16", new Uint16Array(4));
row("float64", new Float64Array(2));
row("bigint64", new BigInt64Array(2));
row("dataview", new DataView(new ArrayBuffer(4)));
class SubU8 extends Uint8Array {}
class SubDV extends DataView {}
row("sub-u8", new SubU8(4));
row("sub-dataview", new SubDV(new ArrayBuffer(4)));
row("arraybuffer", new ArrayBuffer(4));
row("shared", new SharedArrayBuffer(4));
row("buf.buffer", base.buffer);
row("array", [1, 2]);
row("object", { byteLength: 16 });
row("null", null);
row("undefined", undefined);
row("number", 42);
row("string", "abc");

console.log("proto chain", Object.getPrototypeOf(Buffer.prototype) === Uint8Array.prototype);
console.log("ctor chain", Object.getPrototypeOf(Buffer) === Uint8Array);
console.log("fast proto", FastBuffer.prototype === Buffer.prototype);
console.log("proto instanceof", Buffer.prototype instanceof Uint8Array);
console.log("getProto buf", Object.getPrototypeOf(base) === Buffer.prototype);
console.log("toStringTag", (base as any)[Symbol.toStringTag], (new Uint8Array(1) as any)[Symbol.toStringTag]);

// bson 7.x UUID constructor gate
const bytes = Buffer.from(new ArrayBuffer(32), 8, 16).subarray(0, 16);
console.log("uuid gate", ArrayBuffer.isView(bytes) && bytes.byteLength === 16);
