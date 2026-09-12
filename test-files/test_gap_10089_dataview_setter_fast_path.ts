// parity-env: PERRY_GC_FORCE_EVACUATE=1 PERRY_GC_VERIFY_EVACUATION=1 PERRY_GC_PROTECT_FROMSPACE=1
// #10089: DataView's numeric setter fast path must avoid a runtime handle
// scope without changing coercion order, errors, byte order, or shared views.

function errorName(label: string, callback: () => void): void {
  try {
    callback();
    console.log(label, "NO THROW");
  } catch (error) {
    console.log(label, (error as Error).name);
  }
}

function numericRoundTrips(little: boolean): string {
  const view = new DataView(new ArrayBuffer(64));
  view.setInt8(0, -123);
  view.setUint8(1, 250);
  view.setInt16(2, -0x1234, little);
  view.setUint16(4, 0xabcd, little);
  view.setInt32(8, -0x1234567, little);
  view.setUint32(12, 0x89abcdef, little);
  view.setFloat32(16, 1.5, little);
  view.setFloat64(24, -3.25, little);
  return [
    view.getInt8(0),
    view.getUint8(1),
    view.getInt16(2, little),
    view.getUint16(4, little),
    view.getInt32(8, little),
    view.getUint32(12, little),
    view.getFloat32(16, little),
    view.getFloat64(24, little),
  ].join(",");
}

console.log("numeric big-endian", numericRoundTrips(false));
console.log("numeric little-endian", numericRoundTrips(true));

{
  const buffer = new ArrayBuffer(16);
  const bytes = new Uint8Array(buffer);
  const view = new DataView(buffer);
  view.setUint32(0, 0x01020304, false);
  view.setUint32(4, 0x01020304, true);
  view.setFloat32(8, 1.5, false);
  view.setFloat32(12, 1.5, true);
  console.log("endian bytes", Array.from(bytes).join(","));
}

{
  const buffer = new ArrayBuffer(16);
  const words = new Uint8Array(buffer);
  const view = new DataView(buffer, 4, 8);
  view.setUint32(0, 0x01020304, false);
  const throughTypedArray = Array.from(words.slice(4, 8)).join(",");
  words.set([0x05, 0x06, 0x07, 0x08], 8);
  console.log(
    "shared views",
    throughTypedArray,
    view.getUint32(4, false).toString(16),
  );
}

{
  const view = new DataView(new ArrayBuffer(8));
  view.setUint32(0, "17" as any, true);
  view.setUint16(4, { valueOf() { return 0x2345; } } as any, true);
  console.log("ToNumber", view.getUint32(0, true), view.getUint16(4, true));

  errorName("negative offset", () => view.setUint32(-1, 1));
  errorName("out of range", () => view.setUint32(5, 1));
  errorName("throwing valueOf", () => view.setUint32(0, {
    valueOf() { throw new Error("coercion marker"); },
  } as any));
  errorName("numeric BigInt", () => view.setUint32(0, 1n as any));
}

{
  let valueCalls = 0;
  const view = new DataView(new ArrayBuffer(4));
  errorName("negative before value", () => view.setUint32(-1, {
    valueOf() { valueCalls++; return 1; },
  } as any));
  console.log("negative value calls", valueCalls);

  errorName("number value before bounds", () => view.setUint32(4, {
    valueOf() { valueCalls++; return 1; },
  } as any));
  console.log("number value calls", valueCalls);

  errorName("BigInt Number in bounds", () => view.setBigInt64(0, 1 as any));
  errorName("BigInt Number before bounds", () => view.setBigInt64(8, 1 as any));
  errorName("BigInt string then bounds", () => view.setBigInt64(8, "1" as any));
}

{
  const buffer = new ArrayBuffer(8);
  const view = new DataView(buffer);
  errorName("detach during ToNumber", () => view.setUint32(0, {
    valueOf() {
      buffer.transfer();
      return 7;
    },
  } as any, true));
}

{
  let coercions = 0;
  const buffer = new ArrayBuffer(8);
  const mirror = new Uint8Array(buffer);
  new DataView(buffer).setUint32(0, {
    valueOf() {
      coercions++;
      const pressure: object[] = [];
      for (let i = 0; i < 512; i++) pressure.push({ i, text: "move-" + i });
      const collect = (globalThis as any).gc;
      if (typeof collect === "function") collect();
      return 0x01020304;
    },
  } as any, false);
  console.log("moving coercion", coercions, Array.from(mirror.slice(0, 4)).join(","));
}

{
  const view = new DataView(new ArrayBuffer(16));
  view.setBigInt64(0, -2n, false);
  view.setBigUint64(8, 0xfedcba9876543210n, true);
  console.log(
    "BigInt endian",
    view.getBigInt64(0, false),
    view.getBigUint64(8, true).toString(16),
  );
}
