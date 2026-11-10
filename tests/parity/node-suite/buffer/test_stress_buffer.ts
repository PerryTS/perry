// Stress test: Buffer and TypedArray dispatch
// Targets: Buffer.from encodings, Uint8Array(n) construction, index read/write,
// Buffer param byte access, indexOf/includes routing
// Based on bugs: v0.5.36 (#42), v0.5.38 (#47), v0.5.13, v0.5.31 (#38)

// === SECTION: Buffer.from with string encodings ===
const utf8Buf = Buffer.from("hello world", "utf8");
console.log("utf8 length:", utf8Buf.length);
console.log("utf8 toString:", utf8Buf.toString("utf8"));

const hexBuf = Buffer.from("48656c6c6f", "hex");
console.log("hex decode:", hexBuf.toString("utf8"));

const b64Buf = Buffer.from("SGVsbG8gV29ybGQ=", "base64");
console.log("base64 decode:", b64Buf.toString("utf8"));

const b64urlBuf = Buffer.from("SGVsbG8gV29ybGQ", "base64url");
console.log("base64url decode:", b64urlBuf.toString("utf8"));

const asciiBuf = Buffer.from("ASCII test", "ascii");
console.log("ascii length:", asciiBuf.length);
console.log("ascii toString:", asciiBuf.toString("ascii"));

const latin1Buf = Buffer.from("latin1", "latin1");
console.log("latin1 toString:", latin1Buf.toString("latin1"));

// === SECTION: Buffer.from with arrays ===
const fromArr = Buffer.from([72, 101, 108, 108, 111]);
console.log("from array:", fromArr.toString("utf8"));
console.log("from array length:", fromArr.length);

// === SECTION: Buffer.alloc ===
const allocBuf = Buffer.alloc(10);
console.log("alloc length:", allocBuf.length);
console.log("alloc[0]:", allocBuf[0]);
console.log("alloc[9]:", allocBuf[9]);

const allocFill = Buffer.alloc(5, 0x41);
console.log("alloc filled:", allocFill.toString("utf8"));

// === SECTION: Buffer index read/write ===
const rw = Buffer.alloc(8);
rw[0] = 65;  // 'A'
rw[1] = 66;  // 'B'
rw[2] = 67;  // 'C'
rw[7] = 90;  // 'Z'
console.log("rw[0]:", rw[0]);
console.log("rw[1]:", rw[1]);
console.log("rw[2]:", rw[2]);
console.log("rw[7]:", rw[7]);
console.log("rw[3]:", rw[3]);

// === SECTION: Buffer.slice ===
const sliceSrc = Buffer.from("Hello World");
const sliced = sliceSrc.slice(0, 5);
console.log("slice 0-5:", sliced.toString("utf8"));
const sliced2 = sliceSrc.slice(6);
console.log("slice 6-:", sliced2.toString("utf8"));

// === SECTION: Buffer.concat ===
const c1 = Buffer.from("Hello");
const c2 = Buffer.from(" ");
const c3 = Buffer.from("World");
const concatenated = Buffer.concat([c1, c2, c3]);
console.log("concat:", concatenated.toString("utf8"));
console.log("concat length:", concatenated.length);

// === SECTION: Buffer.toString with different encodings ===
const multi = Buffer.from("Hello");
console.log("toString utf8:", multi.toString("utf8"));
console.log("toString hex:", multi.toString("hex"));
console.log("toString base64:", multi.toString("base64"));

// === SECTION: Buffer index access in functions (v0.5.36 bug) ===
function sumBytes(buf: Buffer): number {
  let sum = 0;
  for (let i = 0; i < buf.length; i++) {
    sum += buf[i];
  }
  return sum;
}

const testBuf = Buffer.from([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
console.log("sumBytes:", sumBytes(testBuf));

function readByte(buf: Buffer, idx: number): number {
  return buf[idx];
}
console.log("readByte(0):", readByte(testBuf, 0));
console.log("readByte(5):", readByte(testBuf, 5));
console.log("readByte(9):", readByte(testBuf, 9));

// === SECTION: Buffer.indexOf and includes (v0.5.13 bug) ===
const searchBuf = Buffer.from("Hello World Hello");
console.log("indexOf H:", searchBuf.indexOf(72));
console.log("indexOf W:", searchBuf.indexOf(87));
console.log("includes 72:", searchBuf.includes(72));
console.log("includes 0:", searchBuf.includes(0));

// === SECTION: Uint8Array construction (v0.5.31 bug) ===
const u1 = new Uint8Array(10);
console.log("Uint8Array(10) length:", u1.length);
console.log("Uint8Array(10)[0]:", u1[0]);

const size = 5;
const u2 = new Uint8Array(size);
console.log("Uint8Array(var) length:", u2.length);

const u3 = new Uint8Array(size + 3);
console.log("Uint8Array(expr) length:", u3.length);

// === SECTION: Uint8Array read/write ===
const ua = new Uint8Array(5);
ua[0] = 10;
ua[1] = 20;
ua[2] = 30;
ua[3] = 40;
ua[4] = 50;
console.log("ua[0]:", ua[0]);
console.log("ua[4]:", ua[4]);

let uaSum = 0;
for (let i = 0; i < ua.length; i++) {
  uaSum += ua[i];
}
console.log("ua sum:", uaSum);

// === SECTION: Buffer.copy ===
const src = Buffer.from("Hello World");
const dst = Buffer.alloc(5);
src.copy(dst, 0, 0, 5);
console.log("copy:", dst.toString("utf8"));

// === SECTION: Buffer.fill ===
const fillBuf = Buffer.alloc(10);
fillBuf.fill(65);
console.log("fill A:", fillBuf.toString("utf8"));

// === SECTION: Buffer.equals ===
const eq1 = Buffer.from("hello");
const eq2 = Buffer.from("hello");
const eq3 = Buffer.from("world");
console.log("equals same:", eq1.equals(eq2));
console.log("equals diff:", eq1.equals(eq3));

// === SECTION: Buffer.write ===
const writeBuf = Buffer.alloc(11);
writeBuf.write("Hello");
writeBuf.write(" World", 5);
console.log("write:", writeBuf.toString("utf8"));

// === SECTION: Large buffer operations ===
const largeBuf = Buffer.alloc(10000);
for (let i = 0; i < 10000; i++) {
  largeBuf[i] = i % 256;
}
console.log("large[0]:", largeBuf[0]);
console.log("large[255]:", largeBuf[255]);
console.log("large[9999]:", largeBuf[9999]);

let largeSum = 0;
for (let i = 0; i < 10000; i++) {
  largeSum += largeBuf[i];
}
console.log("large sum:", largeSum);
