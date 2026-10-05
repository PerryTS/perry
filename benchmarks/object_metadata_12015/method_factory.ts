function read(receiver: any): number {
  return receiver.get() + receiver.has();
}
function measure(receiver: any, count: number): number {
  let total = 0;
  for (let i = 0; i < count; i++) {
    const channel = {
      get() { return receiver.value; },
      has() { return receiver.value + 1; },
      set(value: number) { receiver.value = value; },
      remove() { return receiver.value - 1; },
      assert() { return receiver.value === 7; },
    };
    total += read(channel);
  }
  return total;
}
console.log(measure({ value: 7 }, Number(process.argv[2] || 100000)));
