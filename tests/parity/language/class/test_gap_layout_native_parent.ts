import { EventEmitter } from "node:events";
import { Readable, Writable, Duplex } from "node:stream";
class LayoutEmitter extends EventEmitter {
  constructor() { super(); this.marker = 17; }
}
class LayoutReadable extends Readable {
  constructor() { super(); this.marker = 23; }
  _read() { this.push(null); }
}
class LayoutWritable extends Writable {
  constructor() { super(); this.marker = 31; }
  _write(chunk: any, encoding: any, callback: any) { callback(); }
}
class LayoutDuplex extends Duplex {
  constructor() { super(); this.marker = 47; }
  _read() { this.push(null); }
  _write(chunk: any, encoding: any, callback: any) { callback(); }
}
for (const x of [new LayoutEmitter(), new LayoutReadable(), new LayoutWritable(), new LayoutDuplex()]) {
  let count = 0;
  x.on("layout", (n: number) => { count += n; });
  x.emit("layout", x.marker);
  console.log(x.marker, count, Object.hasOwn(x, "_events"),
    Object.hasOwn(x, "marker"), Object.keys(x).includes("marker"),
    ({...x}).marker, JSON.parse(JSON.stringify({marker:x.marker})).marker);
  (x as any)["marker"] += 1;
  console.log(x.marker);
}
