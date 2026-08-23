// Gap test: Buffer operations
// Run: node --experimental-strip-types test_gap_buffer_ops.ts

// --- Buffer.from(string, encoding) ---
console.log("from utf8:", Buffer.from("hello").toString("utf8"));
console.log("from hex:", Buffer.from("48656c6c6f", "hex").toString("utf8"));
console.log("from base64:", Buffer.from("SGVsbG8=", "base64").toString("utf8"));
console.log("from base64url:", Buffer.from("SGVsbG8", "base64url").toString("utf8"));

// --- Buffer.from(array) ---
console.log("from array:", Buffer.from([72, 101, 108, 108, 111]).toString("utf8"));
console.log("from empty:", Buffer.from([]).length);

// --- Buffer.alloc ---
const a1 = Buffer.alloc(5);
console.log("alloc zeros:", a1[0], a1[4]);
console.log("alloc length:", a1.length);

const a2 = Buffer.alloc(3, 65);
console.log("alloc fill:", a2.toString("utf8"));

// --- Buffer.concat ---
const c = Buffer.concat([Buffer.from("Hel"), Buffer.from("lo")]);
console.log("concat:", c.toString("utf8"));
console.log("concat length:", c.length);

const empty = Buffer.concat([]);
console.log("concat empty:", empty.length);

// --- Buffer index access ---
const buf = Buffer.from("ABCDE");
console.log("buf[0]:", buf[0]);
console.log("buf[4]:", buf[4]);

// --- Buffer index write ---
const w = Buffer.alloc(3);
w[0] = 88;
w[1] = 89;
w[2] = 90;
console.log("write:", w.toString("utf8"));

// --- Buffer.slice ---
const s = Buffer.from("Hello World");
console.log("slice 0-5:", s.slice(0, 5).toString("utf8"));
console.log("slice 6:", s.slice(6).toString("utf8"));
console.log("slice neg:", s.slice(-5).toString("utf8"));

// --- Buffer.copy ---
const src = Buffer.from("Hello");
const dst = Buffer.alloc(5);
const copied = src.copy(dst);
console.log("copy:", dst.toString("utf8"));
console.log("copied bytes:", copied);

// --- Buffer.fill ---
const f = Buffer.alloc(5);
f.fill(42);
console.log("fill:", f[0], f[4]);
f.fill(65);
console.log("fill A:", f.toString("utf8"));

// --- Buffer.equals ---
console.log("equals same:", Buffer.from("abc").equals(Buffer.from("abc")));
console.log("equals diff:", Buffer.from("abc").equals(Buffer.from("xyz")));
console.log("equals len:", Buffer.from("ab").equals(Buffer.from("abc")));

// --- Buffer.indexOf ---
const search = Buffer.from("Hello World Hello");
console.log("indexOf H:", search.indexOf(72));
console.log("indexOf o:", search.indexOf(111));
console.log("indexOf missing:", search.indexOf(0));

// --- Buffer.includes ---
console.log("includes H:", search.includes(72));
console.log("includes 0:", search.includes(0));

// --- Buffer.write ---
const wb = Buffer.alloc(11);
wb.write("Hello");
wb.write(" World", 5);
console.log("write:", wb.toString("utf8"));

// --- Buffer.toString with range ---
const range = Buffer.from("Hello World");
console.log("toString 0-5:", range.toString("utf8", 0, 5));
console.log("toString 6-11:", range.toString("utf8", 6, 11));

// --- Buffer.toString hex/base64 ---
const enc = Buffer.from("Hi");
console.log("hex:", enc.toString("hex"));
console.log("base64:", enc.toString("base64"));

// --- Buffer.length for various encodings ---
console.log("utf8 len:", Buffer.from("hello").length);
console.log("emoji len:", Buffer.from("😀").length);
console.log("empty len:", Buffer.from("").length);

// --- Buffer exotic descriptors and accessor reads ---
const descriptorBuffer = Buffer.from([7]);
const byteDescriptor = Object.getOwnPropertyDescriptor(descriptorBuffer, "0");
console.log(
  "byte descriptor:",
  byteDescriptor?.value,
  byteDescriptor?.writable,
  byteDescriptor?.enumerable,
  byteDescriptor?.configurable,
);

const accessorBuffer: any = Buffer.from([1, 2]);
let accessorHits = 0;
Object.defineProperty(accessorBuffer, "computed", {
  enumerable: true,
  configurable: true,
  get() {
    accessorHits++;
    return 41;
  },
});
const computedKey: any = "computed";
console.log("buffer computed getter:", accessorBuffer[computedKey], accessorHits);
console.log("buffer values getter:", Object.values(accessorBuffer).join(","), accessorHits);
console.log(
  "buffer entries getter:",
  JSON.stringify(Object.entries(accessorBuffer)),
  accessorHits,
);

Object.defineProperty(accessorBuffer, "readUInt8", {
  configurable: true,
  get() {
    accessorHits++;
    return function (_offset: number) {
      return this === accessorBuffer ? 99 : -1;
    };
  },
});
console.log("buffer accessor method:", accessorBuffer.readUInt8(0), accessorHits);

const accessorView: any = new DataView(new ArrayBuffer(1));
Object.defineProperty(accessorView, "0", {
  enumerable: true,
  configurable: true,
  get() {
    return 77;
  },
});
const zeroKey: any = 0;
console.log("dataview numeric getter:", accessorView[zeroKey]);

// --- Buffer.isBuffer --- (not yet in codegen: BufferIsBuffer)
// console.log("isBuffer buf:", Buffer.isBuffer(Buffer.from("x")));

// --- Buffer.byteLength --- (not yet in codegen)
// console.log("byteLength ascii:", Buffer.byteLength("hello", "utf8"));
