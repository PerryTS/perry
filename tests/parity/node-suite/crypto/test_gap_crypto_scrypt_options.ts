import { scrypt, scryptSync } from "node:crypto";
import { promisify } from "node:util";

const password = "correct horse battery staple";
const salt = Buffer.from("0123456789abcdef0123456789abcdef", "hex");
const maxmem = 64 * 1024 * 1024;

function expectHex(actual: any, expected: string, label: string): void {
  const hex = actual.toString("hex");
  if (hex !== expected) {
    throw new Error(label + ": expected " + expected + ", got " + hex);
  }
}

expectHex(
  scryptSync(password, salt, 32, { N: 1 << 12, r: 8, p: 1, maxmem }),
  "05f47c22e65fc21d3e11a92222323c577271be35cea9b34b4a6970e2fa3ebf48",
  "sync N",
);
expectHex(
  scryptSync(password, salt, 32, { N: 1 << 14, r: 4, p: 1, maxmem }),
  "cd4b3b2db0af02f65f71c999901e07abba66fd088d65268928a7e971a50651f0",
  "sync r",
);
expectHex(
  scryptSync(password, salt, 32, { N: 1 << 14, r: 8, p: 2, maxmem }),
  "77636ab3d38dd67285438f5694c64bfa9fe81144034a49e7599511136bcfb071",
  "sync p",
);

const scryptAsync = promisify(scrypt);
const asyncResult: any = await scryptAsync(password, salt, 32, {
  N: 1 << 13,
  r: 8,
  p: 1,
  maxmem,
});
expectHex(
  asyncResult,
  "a2d6acfcc30e33aa067080845b9f4790427da3d8f992e651765c095abb7cd276",
  "async N",
);

let memoryError = "no error";
try {
  scryptSync(password, salt, 32, {
    N: 1 << 17,
    r: 8,
    p: 1,
    maxmem: 1024,
  });
} catch (error: any) {
  memoryError = [
    error?.name,
    error?.code,
    String(error?.message).includes("memory limit exceeded"),
  ].join(":");
}
if (memoryError !== "RangeError:ERR_CRYPTO_INVALID_SCRYPT_PARAMS:true") {
  throw new Error("unexpected maxmem result: " + memoryError);
}

console.log("scrypt options: ok");
