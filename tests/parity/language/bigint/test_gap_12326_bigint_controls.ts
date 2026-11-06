// Negative controls: widening BigInt does not weaken coercion or operator errors.
function check(label: string, f: () => unknown) {
  try { console.log(label, String(f())); }
  catch (e) { console.log(label, (e as Error).name); }
}
console.log("numbers", 1000 ** 2, 1024 << 2, 255 >>> 2);
check("fraction", () => BigInt(1.5));
check("infinity", () => BigInt(Infinity));
check("invalid", () => BigInt("0xno"));
check("mixed", () => 1n + (1 as any));
check("unsigned-shift", () => (1n as any) >>> 0n);
check("negative-exponent", () => 2n ** -1n);
check("zero-divisor", () => 1n / 0n);
check("limit-shift", () => 1n << 1073741824n);
check("limit-power", () => 2n ** (1n << 2048n));
console.log("small", BigInt.asIntN(8, 255n), BigInt.asUintN(8, -1n), 1n << -1n, 8n >> -1n);
