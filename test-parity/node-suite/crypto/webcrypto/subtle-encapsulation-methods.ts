import { webcrypto } from "node:crypto";

(process as any).emitWarning = () => undefined;

const subtle = webcrypto.subtle;

const descriptorShape = (desc: any) => ({
  enumerable: desc?.enumerable,
  configurable: desc?.configurable,
  writable: "writable" in desc ? desc.writable : undefined,
  value: typeof desc?.value,
});

async function rejection(label: string, promise: Promise<any>) {
  try {
    await promise;
    console.log(`${label}: no reject`);
  } catch (error: any) {
    console.log(`${label}:`, error.name, error.code ?? "", error.message);
  }
}

function directMissingCall(name: string): Promise<any> {
  switch (name) {
    case "encapsulateBits":
      return subtle.encapsulateBits();
    case "decapsulateBits":
      return subtle.decapsulateBits();
    case "encapsulateKey":
      return subtle.encapsulateKey();
    case "decapsulateKey":
      return subtle.decapsulateKey();
    default:
      throw new Error(`unexpected method ${name}`);
  }
}

for (const name of [
  "encapsulateBits",
  "decapsulateBits",
  "encapsulateKey",
  "decapsulateKey",
] as const) {
  const protoDesc = Object.getOwnPropertyDescriptor(SubtleCrypto.prototype, name);
  const ownDesc = Object.getOwnPropertyDescriptor(subtle, name);
  const fn = (subtle as any)[name];
  console.log(
    `${name} shape:`,
    typeof fn,
    fn.name,
    fn.length,
    !!ownDesc,
    JSON.stringify(descriptorShape(protoDesc)),
  );
  const ret = fn.call(subtle);
  console.log(`${name} missing returns promise:`, ret instanceof Promise);
  await rejection(`${name} missing`, ret);
  const directRet = directMissingCall(name);
  console.log(`${name} direct missing returns promise:`, directRet instanceof Promise);
  await rejection(`${name} direct missing`, directRet);
}
