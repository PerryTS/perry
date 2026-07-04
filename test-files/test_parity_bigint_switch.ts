// Parity: `switch` on a BigInt must match `case Nn` by value, not identity.
function classify(x: bigint): string {
  switch (x) {
    case 0n: return "zero";
    case 1n: return "one";
    case 9007199254740993n: return "big";
    default: return "other";
  }
}
console.log(classify(1n + 0n));
console.log(classify(9007199254740992n + 1n));
console.log(classify(5n));
console.log(new Map<bigint, string>([[2n, "two"]]).get(1n + 1n) ?? "miss");
console.log([3n].includes(1n + 2n));
