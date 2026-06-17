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

console.log(packedF64LoopVersioningChecksum());
