function assertEqual(actual: unknown, expected: unknown, label: string): void {
  if (actual !== expected) {
    throw new Error(`${label}: expected ${String(expected)}, got ${String(actual)}`);
  }
}

class PermissionsBitField {
  static resolve(bits: any): bigint {
    if (typeof bits === "bigint") return bits;
    return BigInt(bits);
  }

  bitfield: bigint;

  constructor(bits: any = 0n) {
    this.bitfield = PermissionsBitField.resolve(bits);
  }

  has(bits: any): boolean {
    const resolved = PermissionsBitField.resolve(bits);
    return (this.bitfield & resolved) === resolved;
  }
}

let bits = PermissionsBitField.resolve("1024");
for (const roleBits of [0n, 8192n]) {
  bits = bits | PermissionsBitField.resolve(roleBits);
}

const permissions = new PermissionsBitField(bits);

console.log("bits", bits, typeof bits);
console.log("hasManageMessages", permissions.has(8192n));
console.log("hasSendMessages", permissions.has(2048n));

assertEqual(bits, 9216n, "bits");
assertEqual(typeof bits, "bigint", "typeof bits");
assertEqual(permissions.has(8192n), true, "has manage messages");
assertEqual(permissions.has(2048n), false, "has send messages");
