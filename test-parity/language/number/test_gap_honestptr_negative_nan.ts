// Negative quiet/signalling payloads must remain Numbers across every raw
// byte ingress, subsequent arithmetic, boxed storage, and collection.
function check(name: string, x: any) {
  const held: any = { value: x };
  const array: any[] = [x];
  if (typeof globalThis.gc === "function") globalThis.gc();
  console.log(name, typeof held.value, Number.isNaN(held.value),
    typeof array[0], Number.isNaN(array[0]));
}
for (const hi of [0xfff10000, 0xfff90000, 0xffff0000, 0x7ff90000]) {
  const bytes = new ArrayBuffer(8);
  const view = new DataView(bytes);
  view.setUint32(0, 0x12345678, true);
  view.setUint32(4, hi, true);
  const typed = new Float64Array(bytes);
  const x = typed[0];
  check("Float64Array", x);
  check("DataView", view.getFloat64(0, true));
  check("Buffer", Buffer.from(bytes).readDoubleLE(0));
  check("arithmetic", x + 1);
  check("negate", -x);
  check("Math.abs", Math.abs(x));
  check("Math.sqrt", Math.sqrt(x));
  // JSON syntax cannot encode NaN; a reviver can supply a byte-derived NaN.
  check("JSON.parse reviver", JSON.parse("0", () => x));
  check("structuredClone", structuredClone({ value: x }).value);
}
check("arithmetic invalid", 0 / 0);
check("Math invalid", Math.sqrt(-1));
