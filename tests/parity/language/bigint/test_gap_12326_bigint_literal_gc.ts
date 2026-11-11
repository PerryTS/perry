// parity-node-argv: --expose-gc
// Wide literals and retained variable-length leaf allocations survive collection.
const literal = 0x10000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000001n;
const values: bigint[] = [];
for (let i = 0; i < 4000; i++) values.push((1n << BigInt(2048 + i % 200)) + BigInt(i));
(globalThis as any).gc?.();
let ok = 0;
for (let i = 0; i < values.length; i++) {
  const value = values[i];
  if ((value & 0xffffn) === BigInt(i) && value >> BigInt(2048 + i % 200) === 1n) ok++;
}
console.log("literal", literal > (1n << 1024n), literal - 1n === 1n << 1024n);
console.log("retained", ok);
