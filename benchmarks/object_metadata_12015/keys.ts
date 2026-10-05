// Parameter receiver: Object.keys must inspect the runtime shape.
function measure(receiver: any, count: number): number {
  let total = 0;
  for (let i = 0; i < count; i++) total += Object.keys(receiver).length;
  return total;
}
const receiver: any = { alpha: 1, beta: 2, gamma: 3, delta: 4 };
Object.defineProperty(receiver, "hidden", { value: 9, enumerable: false });
console.log(measure(receiver, Number(process.argv[2] || 100000)));
