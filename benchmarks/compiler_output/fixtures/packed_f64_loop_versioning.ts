function packedF64LoopVersioningChecksum(): number {
  const values: number[] = [1.5, 2.25, 3.75, 4.5, 6.0];

  let sum = 0;
  for (let i = 0; i < values.length; i++) {
    sum = sum + values[i];
  }

  for (let i = 0; i < values.length; i++) {
    values[i] = values[i] * 2 + i;
  }

  let rewritten = 0;
  for (let i = 0; i < values.length; i++) {
    rewritten = rewritten + values[i];
  }

  return sum + rewritten;
}

function dynamicRhsPackedStore(value: number): number {
  const values: number[] = [1, 2, 3];

  for (let i = 0; i < values.length; i++) {
    values[i] = value;
  }

  let score = 0;
  for (let i = 0; i < values.length; i++) {
    score += values[i];
  }
  return score;
}

const rhsFromAny: any = 2;

console.log(
  packedF64LoopVersioningChecksum() + dynamicRhsPackedStore(rhsFromAny as number)
);
