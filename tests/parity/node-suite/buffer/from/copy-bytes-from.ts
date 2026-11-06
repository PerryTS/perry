import { Buffer } from "node:buffer";

function show(label: string, value: Buffer) {
  console.log(`${label}:`, `${value.toString("hex")}/${value.length}`);
}

function showError(label: string, fn: () => unknown) {
  try {
    fn();
    console.log(`${label}:`, "no throw");
  } catch (err: any) {
    console.log(`${label}:`, err?.name, err?.code || "no-code");
  }
}

const bytes = new Uint8Array([1, 2, 3, 4]);
const u8All = Buffer.copyBytesFrom(bytes);
const u8Range = Buffer.copyBytesFrom(bytes, 1, 2);
show("u8 all", u8All);
show("u8 range", u8Range);
const boundCopy = Buffer.copyBytesFrom;
show("bound u8", boundCopy(bytes, 0, 2));
const boundIndexed = boundCopy(bytes, 0, 1);
console.log("bound index:", String(boundIndexed[0]));

const wide = new Uint16Array([0x0102, 0x0304, 0x0506]);
const u16All = Buffer.copyBytesFrom(wide);
const u16Range = Buffer.copyBytesFrom(wide, 1, 1);
show("u16 all", u16All);
show("u16 range", u16Range);

const copy = Buffer.copyBytesFrom(bytes);
bytes[0] = 99;
console.log("copy independent:", String(copy[0]), String(bytes[0]));

const lengthClamp = Buffer.copyBytesFrom(bytes, 1, 99);
show("length clamp", lengthClamp);

showError("dataview", () => Buffer.copyBytesFrom(new DataView(new ArrayBuffer(2)) as any));
showError("arraybuffer", () => Buffer.copyBytesFrom(new ArrayBuffer(2) as any));
showError("negative offset", () => Buffer.copyBytesFrom(bytes, -1));
showError("fractional offset", () => Buffer.copyBytesFrom(bytes, 1.5));
showError("negative length", () => Buffer.copyBytesFrom(bytes, 0, -1));
showError("fractional length", () => Buffer.copyBytesFrom(bytes, 0, 1.5));
