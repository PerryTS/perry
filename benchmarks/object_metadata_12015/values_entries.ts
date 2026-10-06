function measure(receiver: any, count: number): number {
  let total = 0;
  for (let i = 0; i < count; i++) {
    const values = Object.values(receiver);
    const entries = Object.entries(receiver);
    total += values[0] + entries[1][1];
  }
  return total;
}
console.log(measure({ alpha: 1, beta: 2, gamma: 3, delta: 4 }, Number(process.argv[2] || 100000)));
