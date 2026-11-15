// process.stdout used without any stream import: write, listener methods and
// the stream-shaped members a program can observe.
process.stdout.write("write 1\n");
const ret = process.stdout.write("write 2\n");
console.log("write returned", ret);
process.stdout.on("error", () => {});
console.log("writable", process.stdout.writable, typeof process.stdout.once, typeof process.stdout.emit);
process.stderr.write("");
console.log("stderr writable", process.stderr.writable);
