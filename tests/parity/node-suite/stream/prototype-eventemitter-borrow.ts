import StreamModule from "node:stream";
const Stream: any = StreamModule;
let failures = 0;
if (typeof Stream.prototype !== "object" || typeof Stream.prototype.on !== "function") {
  failures++;
} else {
  const a: any = {};
  let gotA: any = null;
  Stream.prototype.on.call(a, "data", (x: any) => { gotA = x; });
  Stream.prototype.emit.call(a, "data", 11);
  if (gotA !== 11) failures++;
}
const SS: any = Stream.Stream;
if (typeof SS.prototype.on !== "function" || typeof SS.prototype.removeListener !== "function") {
  failures++;
} else {
  const b: any = {};
  let count = 0;
  const fn = () => { count++; };
  SS.prototype.on.call(b, "x", fn);
  SS.prototype.emit.call(b, "x");
  SS.prototype.removeListener.call(b, "x", fn);
  SS.prototype.emit.call(b, "x");
  if (count !== 1) failures++;
}
if (failures !== 0) throw new Error("Stream.prototype EventEmitter borrow regression failed");
console.log("stream proto ee borrow ok");
