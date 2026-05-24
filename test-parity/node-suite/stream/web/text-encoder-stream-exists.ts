// TextEncoderStream is a global (per WHATWG spec); check it exists and
// constructs.
console.log("TextEncoderStream typeof:", typeof (globalThis as any).TextEncoderStream);
console.log("TextDecoderStream typeof:", typeof (globalThis as any).TextDecoderStream);
const Cls = (globalThis as any).TextEncoderStream;
if (typeof Cls === "function") {
  const tes = new Cls();
  console.log("constructed:", typeof tes === "object");
  console.log("has readable:", "readable" in tes);
  console.log("has writable:", "writable" in tes);
}
