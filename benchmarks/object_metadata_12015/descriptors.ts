function measure(receiver: any, count: number): number {
  let total = 0;
  for (let i = 0; i < count; i++) {
    const descriptor = Object.getOwnPropertyDescriptor(receiver, "beta");
    total += descriptor.value + Object.getOwnPropertyNames(receiver).length;
  }
  return total;
}
console.log(measure({ alpha: 1, beta: 2, gamma: 3, delta: 4 }, Number(process.argv[2] || 100000)));
