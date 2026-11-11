// What a program can observe of the errors Buffer methods throw for a bad
// size, argument or range: real error objects with Node's name and code.
function show(label: string, f: () => unknown): void {
  try {
    f();
    console.log(label, "no throw");
  } catch (e: any) {
    console.log(label, "name:", e.name, "code:", e.code);
    console.log(label, "message:", e.message);
    console.log(
      label,
      "instanceof Error/RangeError/TypeError:",
      e instanceof Error,
      e instanceof RangeError,
      e instanceof TypeError,
    );
    console.log(label, "keys:", JSON.stringify(Object.keys(e)));
    console.log(label, "String:", String(e));
    console.log(label, "stack is a string:", typeof e.stack === "string");
    console.log(label, "tag:", Object.prototype.toString.call(e));
  }
}

show("alloc(-1)", () => Buffer.alloc(-1));
show("concat([1])", () => Buffer.concat([1 as any]));
show("swap16 odd", () => Buffer.from([1, 2, 3]).swap16());
show("swap32 odd", () => Buffer.from([1, 2, 3]).swap32());
show("compare targetEnd", () => Buffer.from([1, 2]).compare(Buffer.from([1, 2]), 0, 10));
show("readUInt8 past end", () => Buffer.alloc(4).readUInt8(10));
show("readDoubleLE past end", () => Buffer.alloc(4).readDoubleLE(0));
