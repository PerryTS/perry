function numericArraysChecksum(): number {
  const values: number[] = [1.25, 2.5, 4.0];
  values.push(8.0);

  let sum = 0;
  for (let i = 0; i < values.length; i++) {
    sum = sum + values[i];
  }

  values[1] = values[0] + values[3];
  return sum + values[1];
}

console.log(numericArraysChecksum());
