import { generatePrimeSync, checkPrimeSync, generatePrime, checkPrime } from "node:crypto";
import { Buffer } from "node:buffer";

function generatePrimeAsync(size: number, options: any = {}): Promise<Buffer> {
  return new Promise((resolve, reject) => {
    const ret = generatePrime(size, options, (err: Error | null, prime: Buffer) => {
      if (err) reject(err);
      else resolve(prime);
    });
    console.log("generatePrime ret undefined:", ret === undefined);
  });
}

function generatePrimeBigIntAsync(size: number): Promise<bigint> {
  return new Promise((resolve, reject) => {
    const ret = generatePrime(size, { bigint: true }, (err: Error | null, prime: bigint) => {
      if (err) reject(err);
      else resolve(prime);
    });
    console.log("generatePrime bigint ret undefined:", ret === undefined);
  });
}

function checkPrimeAsync(candidate: Buffer | bigint, options: any = {}): Promise<boolean> {
  return new Promise((resolve, reject) => {
    const ret = checkPrime(candidate, options, (err: Error | null, ok: boolean) => {
      if (err) reject(err);
      else resolve(ok);
    });
    console.log("checkPrime ret undefined:", ret === undefined);
  });
}

const prime = generatePrimeSync(32);
console.log("prime len:", Buffer.from(prime).length);
console.log("prime check sync:", checkPrimeSync(prime));

const safePrime = generatePrimeSync(32, { safe: true });
console.log("safe prime check sync:", checkPrimeSync(safePrime));

const add = Buffer.from([12]);
const rem = Buffer.from([11]);
const constrained = generatePrimeSync(32, { add, rem });
console.log("constrained check sync:", checkPrimeSync(constrained));
console.log("constrained remainder:", Buffer.from(constrained).readUInt32BE() % 12);

const asyncPrime = await generatePrimeAsync(32, {});
console.log("async prime len:", Buffer.from(asyncPrime).length);
console.log("async check sync:", checkPrimeSync(asyncPrime));
console.log("async check callback:", await checkPrimeAsync(asyncPrime, {}));
console.log("known composite false:", checkPrimeSync(Buffer.from([0x00, 0x00, 0x00, 0x15])));

for (const size of [65, 129, 256]) {
  const largePrime = generatePrimeSync(size);
  console.log("large prime len:", size, Buffer.from(largePrime).length);
  console.log("large prime check:", size, checkPrimeSync(largePrime));
}

console.log("bigint candidates:", checkPrimeSync(17n), checkPrimeSync(21n));
const bigintPrime = generatePrimeSync(64, { bigint: true });
console.log(
  "bigint prime sync:",
  typeof bigintPrime,
  String(bigintPrime).length > 0,
  checkPrimeSync(bigintPrime),
);
const asyncBigintPrime = await generatePrimeBigIntAsync(64);
console.log(
  "bigint prime async:",
  typeof asyncBigintPrime,
  String(asyncBigintPrime).length > 0,
  checkPrimeSync(asyncBigintPrime),
);
console.log("bigint check callback:", await checkPrimeAsync(17n, {}));
