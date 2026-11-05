import * as tty from "node:tty";
function show(label: string, action: () => unknown): void {
  try { action(); console.log(label, "no throw"); }
  catch (e: any) {
    console.log(label, e instanceof RangeError, e instanceof TypeError);
    console.log(label, e.name, e.code, e.message);
    console.log(label, JSON.stringify(Object.keys(e)), Object.prototype.toString.call(e));
  }
}
show("alloc", () => Buffer.alloc(-1));
show("concat", () => Buffer.concat([1 as any]));
show("swap", () => Buffer.from([1, 2, 3]).swap16());
show("out-of-range", () => Buffer.alloc(4).readUInt8(10));
show("out-of-bounds", () => Buffer.alloc(4).readDoubleLE(0));
show("tty", () => new tty.ReadStream(-1));
