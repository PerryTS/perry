// #11430: Buffer methods must accept a SHORT string value. Since #10762,
// `String(n)`, `` `${n}` `` and `n.toString()` return small numbers as SSO
// (inline short strings): the characters live in the NaN-box itself, with no
// heap header behind them. `buf.write(s)` and the `<encoding>Write` family
// masked the value into a pointer anyway and read the inline bytes as an
// address — a segfault. node-postgres binds every parameter as
// `writer.addInt32PrefixedString(String(value))` → `buffer.write(...)`, so any
// parameterised pg query with a short value crashed. `indexOf` /
// `lastIndexOf` with an SSO needle returned -1.
const s = String(7);
const t = String(123456);
const neg = (-42).toString();
const tpl = `${3.5}`;

const b = Buffer.alloc(12);
console.log("write:", b.write(s), b.write(t, 1), b.write(s, 8, 2), b.write(t, 9, 2, "latin1"), b.toString("hex"));
console.log("write neg/tpl:", Buffer.alloc(6).write(neg, 0, "utf8"), Buffer.alloc(6).write(tpl, 2));
console.log("utf8Write:", Buffer.alloc(4).utf8Write(s, 1), "latin1Write:", Buffer.alloc(4).latin1Write(t, 0, 2));
console.log("hexWrite:", Buffer.alloc(4).hexWrite(String(12), 0));
console.log("indexOf:", Buffer.from("xx7yy7").indexOf(s), Buffer.from("xx7yy7").lastIndexOf(s), Buffer.from("x123456").includes(t));
console.log("indexOf enc:", Buffer.from("xx7yy").indexOf(s, 0, "latin1"));
console.log("from/byteLength/fill:", Buffer.from(t).toString("hex"), Buffer.byteLength(s), Buffer.alloc(3).fill(s).toString());

// node-postgres' Writer.addInt32PrefixedString (pg-protocol 1.x), verbatim shape.
class Writer {
  buffer = Buffer.allocUnsafe(16);
  offset = 5;
  ensure(size: number) {
    if (this.buffer.length - this.offset < size) {
      const old = this.buffer;
      this.buffer = Buffer.allocUnsafe(old.length + (old.length >> 1) + size);
      old.copy(this.buffer);
    }
  }
  addInt32PrefixedString(value: string) {
    const len = Buffer.byteLength(value);
    this.ensure(4 + len);
    const buffer = this.buffer;
    let offset = this.offset;
    buffer[offset++] = (len >>> 24) & 0xff;
    buffer[offset++] = (len >>> 16) & 0xff;
    buffer[offset++] = (len >>> 8) & 0xff;
    buffer[offset++] = len & 0xff;
    buffer.write(value, offset, "utf-8");
    this.offset = offset + len;
    return this;
  }
}
const w = new Writer();
for (const v of [7, 42, 123456, -1, 0]) w.addInt32PrefixedString(String(v));
console.log("pg bind:", w.buffer.subarray(5, w.offset).toString("hex"));
