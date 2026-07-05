function dynamicFractionalArrayIndexChecksum(seed: number): number {
  const values: number[] = [10, 20, 30];
  const key: number = seed + 0.5;

  values[key] = 99;

  return values[key] + values[1] + values.length;
}

console.log(dynamicFractionalArrayIndexChecksum(1));
