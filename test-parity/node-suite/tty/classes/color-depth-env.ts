import tty from "node:tty";

const out: any = process.stdout;
console.log("write stream:", out instanceof tty.WriteStream || typeof out.getColorDepth === "function");
if (typeof out.getColorDepth === "function") {
  console.log("color depth default:", typeof out.getColorDepth());
  console.log("has colors 2:", typeof out.hasColors(2));
}
const input: any = process.stdin;
console.log("read stream raw type:", typeof input.isRaw);
