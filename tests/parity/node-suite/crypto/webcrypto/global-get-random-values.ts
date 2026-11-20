// #13313: `globalThis.crypto.getRandomValues` threw "crypto.webcrypto.
// getRandomValues is not supported" in a program that never names the crypto
// module, because only that naming installs the crypto dispatch bucket. Keep
// using `globalThis.crypto` only: a bare `crypto.x(...)` call or a
// `node:crypto` import would install the bucket and hide the bug.
const c = globalThis.crypto;

const u = new Uint8Array(16);
console.log("direct getRandomValues:", c.getRandomValues(u) === u, u.some((b) => b !== 0));

const i = new Int32Array(8);
const fill = c.getRandomValues;
console.log("extracted getRandomValues:", fill.call(c, i) === i, i.some((v) => v !== 0));

try {
  fill.call({}, new Uint8Array(4));
  console.log("foreign receiver: no throw");
} catch (e) {
  console.log("foreign receiver:", (e as any).code);
}
