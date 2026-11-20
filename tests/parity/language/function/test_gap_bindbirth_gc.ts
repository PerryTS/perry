// Retained inline partials must survive copying minors with their receivers.
function sum(this: { v: number }, a: { v: number }, b: { v: number }, n: number) {
  return this.v + a.v + b.v + n;
}
const retained: any[] = [];
let checksum = 0;
for (let i = 0; i < 3000; i++) {
  const receiver = { v: i };
  const first = { v: i + 1 };
  const second = { v: i + 2 };
  const bound = sum.bind(receiver, first, second, i);
  retained[i % 128] = bound;
  const pressure = { i, text: 'pressure' + i, values: [i, first, second] };
  checksum += bound() + pressure.i;
}
for (let i = 0; i < 20000; i++) {
  const pressure = { i, text: 'later' + i };
  if (pressure.i < 0) console.log(pressure.text);
}
let later = 0;
for (const bound of retained) {
  later += bound.call({ v: -1 });
  if (bound.name !== 'bound sum' || bound.length !== 0) throw new Error('metadata');
}
console.log(checksum, later, retained.length);
