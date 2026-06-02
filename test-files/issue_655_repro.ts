interface State { pending: number[]; }

const m = new Map<number, State>();
m.set(1, { pending: [10, 20, 30] });
const s = m.get(1)!;

console.log('typeof shift:  ', typeof s.pending.shift);
console.log('typeof splice: ', typeof s.pending.splice);
console.log('typeof pop:    ', typeof s.pending.pop);
console.log('typeof slice:  ', typeof s.pending.slice);

const x = s.pending.shift();
console.log('shift ret:', x, 'len after:', s.pending.length);

s.pending.length = 0;
console.log('after length=0:', s.pending.length);

m.set(2, { pending: [10, 20, 30] });
const s2 = m.get(2)!;
const arr = s2.pending;
console.log('aliased typeof shift:', typeof arr.shift);
console.log('aliased shift ret:', arr.shift());
console.log('back at struct:', s2.pending);
