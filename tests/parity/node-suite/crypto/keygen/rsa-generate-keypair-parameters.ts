import { Buffer } from "node:buffer";
import * as crypto from "node:crypto";
import { generateKeyPair, generateKeyPairSync } from "node:crypto";

const jwk = { format: "jwk" } as const;

function bitLength(n: string): number {
  return BigInt("0x" + Buffer.from(n, "base64url").toString("hex")).toString(2).length;
}

function size(key: any): string {
  return `${bitLength(key.n)} ${Buffer.from(key.e, "base64url").toString("hex")}`;
}

function attempt(label: string, run: () => string) {
  try {
    console.log(label, run());
  } catch (error: any) {
    console.log(label, error instanceof TypeError, error instanceof RangeError, error.name, error.code, error.message);
  }
}

function sync(options: any): string {
  const pair = generateKeyPairSync("rsa", { ...options, publicKeyEncoding: jwk, privateKeyEncoding: jwk } as any) as any;
  return `${size(pair.publicKey)} private ${size(pair.privateKey)}`;
}

// Sizes and exponents.
attempt("3072", () => sync({ modulusLength: 3072 }));
attempt("512", () => sync({ modulusLength: 512 }));
attempt("odd below 2048", () => sync({ modulusLength: 1023 }));
attempt("odd above 2048", () => sync({ modulusLength: 2049 }));
attempt("odd above 2048, short exponent", () => sync({ modulusLength: 2049, publicExponent: 17 }));
attempt("literal 65539", () => sync({ modulusLength: 1024, publicExponent: 65539 }));
for (const publicExponent of [undefined, null, 3, 5, 4294967295]) {
  attempt(`exponent ${publicExponent}`, () => sync({ modulusLength: 1024, publicExponent }));
}
const type: string = ["rsa", "pss"].slice(0, 1).join("");
attempt("dynamic type", () =>
  size((generateKeyPairSync as any)(type, { modulusLength: 1024, publicExponent: 3, publicKeyEncoding: jwk, privateKeyEncoding: jwk }).publicKey));
attempt("captured function", () => {
  const captured: any = (crypto as any)["generateKey" + "PairSync"];
  return size(captured("rsa", { modulusLength: 1024, publicExponent: 3, publicKeyEncoding: jwk, privateKeyEncoding: jwk }).publicKey);
});
attempt("rsa-pss", () => {
  const { publicKey } = generateKeyPairSync("rsa-pss" as any, {
    modulusLength: 1024,
    publicExponent: 3,
    publicKeyEncoding: { type: "spki", format: "der" },
    privateKeyEncoding: { type: "pkcs8", format: "pem" },
  } as any) as any;
  // The 128-byte modulus INTEGER (with its sign byte) and the exponent 3.
  const hex = (Buffer.isBuffer(publicKey) ? publicKey : Buffer.from(String(publicKey).replace(/-----[^-]+-----|\s/g, ""), "base64")).toString("hex");
  return `${hex.includes("02818100")} ${hex.endsWith("020103")}`;
});

// Rejected options.
for (const options of [undefined, null, 5, "x", []] as any[]) {
  attempt(`options ${JSON.stringify(options)}`, () => (generateKeyPairSync("rsa", options), "generated"));
}
attempt("options symbol", () => (generateKeyPairSync("rsa", Symbol("x") as any), "generated"));
attempt("options omitted", () => ((generateKeyPairSync as any)("rsa"), "generated"));
for (const modulusLength of [undefined, null, "2048", 2048n, true, {}, NaN, Infinity, 2048.5, 1e-7, -1, 4294967296, -4294967297, 5e9, 1e20, 1e21, 0, 8, 511, 1073741826, 2147483648]) {
  attempt(`modulusLength ${String(modulusLength)}`, () => sync({ modulusLength }));
}
for (const publicExponent of ["3", 3n, true, {}, new Uint8Array([1, 0, 1]), NaN, 3.5, -1, 4294967296, 0, 1, 2, 4, 65536]) {
  attempt(`publicExponent ${String(publicExponent)}`, () => sync({ modulusLength: 1024, publicExponent }));
}
attempt("both", () => sync({ modulusLength: 8, publicExponent: 4 }));
attempt("rsa-pss", () => ((generateKeyPairSync as any)("rsa-pss", { modulusLength: 2048, publicExponent: 4 }), "generated"));

const order: string[] = [];
attempt("read order", () => {
  generateKeyPairSync("rsa", {
    get publicExponent() { order.push("publicExponent"); return 4; },
    get modulusLength() { order.push("modulusLength"); return 1024; },
  } as any);
  return "generated";
});
console.log(order.join());

// The callback form throws what it can validate and delivers the rest.
attempt("callback, invalid type", () => {
  generateKeyPair("rsa", { modulusLength: "x" } as any, () => console.log("unreachable"));
  return "returned";
});
for (const options of [{ modulusLength: 1024, publicExponent: 3 }, { modulusLength: 8 }, { modulusLength: 1024, publicExponent: 4 }, { modulusLength: 1024, publicExponent: 0 }]) {
  await new Promise<void>((resolve) => {
    let returned = false;
    generateKeyPair("rsa", { ...options, publicKeyEncoding: jwk, privateKeyEncoding: jwk } as any, (error: any, publicKey: any, privateKey: any) => {
      console.log("callback", JSON.stringify(options), returned, error && [error.name, error.code, error.message].join(" / "));
      console.log(publicKey ? size(publicKey) : publicKey, privateKey ? size(privateKey) : privateKey);
      resolve();
    });
    returned = true;
  });
}
