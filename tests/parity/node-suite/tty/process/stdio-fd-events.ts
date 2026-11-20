// Node's tty stdin tests exercise stream event methods; under the parity
// harness stdin/stdout/stderr are pipes, but they still expose stream fds and
// EventEmitter-style methods.
console.log("stdin fd number:", typeof process.stdin.fd === "number");
console.log("stdout fd number:", typeof process.stdout.fd === "number");
console.log("stderr fd number:", typeof process.stderr.fd === "number");
console.log("stdin emit function:", typeof process.stdin.emit === "function");
console.log("stdout on function:", typeof process.stdout.on === "function");
console.log("stdout once function:", typeof process.stdout.once === "function");
try {
  process.stdin.emit("end");
  console.log("stdin emit end ok:", true);
} catch (e: any) {
  console.log("stdin emit end error:", e?.name || "Error");
}
try {
  const ret = process.stdout.on("resize", () => {});
  console.log("stdout on returns self:", ret === process.stdout);
} catch (e: any) {
  console.log("stdout on error:", e?.name || "Error");
}
