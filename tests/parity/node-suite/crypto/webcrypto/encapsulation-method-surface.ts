import { webcrypto } from "node:crypto";

// Node 26 marks these WebCrypto helpers as experimental. Keep the fixture
// focused on stdout API parity.
(process as any).emitWarning = () => undefined;

const subtle = webcrypto.subtle as any;

const descriptorShape = (desc: any) => ({
  enumerable: desc?.enumerable,
  configurable: desc?.configurable,
  writable: "writable" in desc ? desc.writable : undefined,
  value: typeof desc?.value,
});

const rejectionShape = async (label: string, fn: () => Promise<any>) => {
  try {
    const result = fn();
    console.log(`${label} promise:`, !!result && typeof (result as any).then === "function");
    await result;
    console.log(`${label}: resolved`);
  } catch (error: any) {
    console.log(`${label}:`, error.name, error.code ?? "");
  }
};

const rejectionName = async (label: string, fn: () => Promise<any>) => {
  try {
    const result = fn();
    console.log(`${label} promise:`, !!result && typeof (result as any).then === "function");
    await result;
    console.log(`${label}: resolved`);
  } catch (error: any) {
    console.log(`${label}:`, error.name);
  }
};

for (const [name, length] of [
  ["encapsulateBits", 2],
  ["decapsulateBits", 3],
  ["encapsulateKey", 5],
  ["decapsulateKey", 6],
] as const) {
  const fn = subtle[name];
  const protoDesc = Object.getOwnPropertyDescriptor(SubtleCrypto.prototype, name);

  console.log(`${name} typeof:`, typeof fn);
  console.log(`${name} name:`, fn?.name);
  console.log(`${name} length:`, fn?.length);
  console.log(`${name} desc:`, JSON.stringify(descriptorShape(protoDesc)));
  console.log(`${name} own desc missing:`, Object.getOwnPropertyDescriptor(subtle, name) === undefined);
  await rejectionShape(`${name} direct missing`, () => subtle[name]());
  await rejectionShape(`${name} captured missing`, () => fn.call(subtle));
  await rejectionShape(`${name} bare invalid this`, () => fn());
  console.log(`${name} expected length:`, length);
}

const aesKey = await webcrypto.subtle.generateKey(
  { name: "AES-GCM", length: 128 },
  true,
  ["encrypt", "decrypt"],
);

await rejectionName(
  "encapsulateBits unsupported algorithm",
  () => subtle.encapsulateBits("AES-GCM", aesKey),
);
