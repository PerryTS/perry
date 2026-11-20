import tty from "node:tty";

// WriteStream color helpers are available without requiring a real TTY-backed
// stream instance.
console.log("hasColors function:", typeof tty.WriteStream.prototype?.hasColors === "function");
console.log("getColorDepth function:", typeof tty.WriteStream.prototype?.getColorDepth === "function");
try {
  console.log("hasColors boolean:", typeof tty.WriteStream.prototype.hasColors() === "boolean");
} catch (e: any) {
  console.log("hasColors error:", e?.name || "Error");
}
try {
  console.log("colorDepth valid:", [1, 4, 8, 24].includes(tty.WriteStream.prototype.getColorDepth()));
} catch (e: any) {
  console.log("colorDepth error:", e?.name || "Error");
}
