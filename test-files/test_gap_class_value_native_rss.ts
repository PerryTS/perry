// A property read on a native-class instance (fs.Stats, URL, Date, Buffer)
// reaches the instance's class prototype, which is linked from the class's
// function object in the per-agent class-value table. That table was indexed
// by the raw class id, so the first native class with a reserved id
// (fs.Stats is 0xFFFF_0070) grew its page directory to 16M entries: one
// `stats.mtime` read added 128 MiB of zero-filled RSS. The table is now
// indexed by the id's offset within its band.
//
// Checks: the reads add under 1 MB of RSS, and their values match node.

import * as fs from "fs";

const rssMb = () => process.memoryUsage().rss / 1048576;

// Untyped helpers: the reads go through the generic property path.
function readMtime(o: any): any {
  return o.mtime;
}
function readAtime(o: any): any {
  return o.atime;
}
function readSize(o: any): any {
  return o.size;
}
function readIsFile(o: any): any {
  return o.isFile;
}
function readHostname(o: any): any {
  return o.hostname;
}
function readPathname(o: any): any {
  return o.pathname;
}
function readSearch(o: any): any {
  return o.search;
}
function readGetTime(o: any): any {
  return o.getTime;
}
function readLength(o: any): any {
  return o.length;
}
function readByteLength(o: any): any {
  return o.byteLength;
}

// Create every receiver before the baseline: only the reads are measured.
const st = fs.statSync(".");
const url = new URL("https://example.com:8080/a/b?q=1#h");
const date = new Date(Date.UTC(2020, 1, 29, 12, 0, 0));
const buf = Buffer.from("hello, world");

const before = rssMb();
let sink = 0;
for (let i = 0; i < 1000; i++) {
  const m = readMtime(st);
  const a = readAtime(st);
  sink += m instanceof Date ? 1 : 0;
  sink += a instanceof Date ? 1 : 0;
  sink += typeof readSize(st) === "number" ? 1 : 0;
  sink += typeof readIsFile(st) === "function" ? 1 : 0;
  sink += readHostname(url).length;
  sink += readPathname(url).length;
  sink += readSearch(url).length;
  sink += typeof readGetTime(date) === "function" ? 1 : 0;
  sink += readLength(buf) + readByteLength(buf);
}
const growth = rssMb() - before;

console.log("reads:", sink);
console.log("rss growth under 1 MB:", growth < 1);

const m = readMtime(st);
console.log("stats.mtime is a Date:", m instanceof Date);
console.log("stats.mtime matches mtimeMs:", Math.abs(m.getTime() - st.mtimeMs) < 1);
console.log("stats.atime matches atimeMs:", Math.abs(readAtime(st).getTime() - st.atimeMs) < 1);
console.log("stats.isFile():", st.isFile(), "isDirectory():", st.isDirectory());
console.log("url:", readHostname(url), readPathname(url), readSearch(url), url.port, url.hash);
console.log("date:", readGetTime(date).call(date), date.toISOString());
console.log("buffer:", readLength(buf), readByteLength(buf), buf.toString("utf8", 0, 5));
