import { Buffer } from 'node:buffer';

function readAcrossCollections(bytes: Uint8Array): void {
  const roots: any[] = [];
  for (let n = 0; n < 64; n++) {
    roots[n & 7] = { n, bytes };
    if (bytes.length !== 131072 || bytes[0] + bytes[bytes.length - 1] !== 84)
      throw new Error('Native parameter read changed across collection');
  }
  if (roots[7].n !== 63 || roots[7].bytes !== bytes)
    throw new Error('Native parameter root did not survive collection');
}

function read(bytes: Uint8Array): string {
  let sum = 0;
  for (let i = 0; i < bytes.length; i++) sum += bytes[i];
  return bytes.length + ' ' + sum;
}

function detach(bytes: Uint8Array): void {
  let before = 0;
  for (let i = 0; i < 8; i++) before += bytes[i];
  const ab = bytes.buffer;
  const moved = structuredClone(ab, { transfer: [ab] });
  let after = 0;
  for (let i = 0; i < bytes.length; i++) after += bytes[i];
  console.log('detach', before, bytes.length, after, String(bytes[0]), new Uint8Array(moved)[131071]);
}

const bytes = Buffer.alloc(131072, 42);
readAcrossCollections(bytes);
console.log('native', read(bytes));
console.log('view', read(bytes.subarray(13, 1025)));
detach(bytes);
