import * as tty from "node:tty";
function show(label: string, action: () => unknown): void {
  try { action(); console.log(label, "no throw"); }
  catch (e: any) {
    console.log(label, e instanceof RangeError, e instanceof TypeError);
    console.log(label, e.name, e.code, e.message);
    console.log(label, "keys", JSON.stringify(Object.keys(e)), Object.prototype.toString.call(e));
    console.log(label, "entries", JSON.stringify(Object.entries(e)));
    const inherited: string[] = [];
    for (const key in e) inherited.push(key);
    console.log(label, "for-in", JSON.stringify(inherited));
    console.log(label, "json", JSON.stringify(e));
    console.log(label, "spread", JSON.stringify({ ...e }));
    console.log(label, "own-names", JSON.stringify(Object.getOwnPropertyNames(e)));
    console.log(label, "own-message", e.hasOwnProperty("message"));
    console.log(label, "stack-string", typeof e.stack === "string");
    console.log(label, "string", String(e));
    console.log(label, "error", e instanceof Error);
  }
}
show("alloc", () => Buffer.alloc(-1));
show("concat", () => Buffer.concat([1 as any]));
show("swap", () => Buffer.from([1, 2, 3]).swap16());
show("out-of-range", () => Buffer.alloc(4).readUInt8(10));
show("out-of-bounds", () => Buffer.alloc(4).readDoubleLE(0));
show("tty", () => new tty.ReadStream(-1));
