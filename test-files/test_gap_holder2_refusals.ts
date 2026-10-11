// Warm and saturate one computed site, then change the proof underneath it.
function read(o: any, key: any): any { return o[key]; }
const values: any[] = [];
const holder: any = { answer: 7 };
const key = Symbol('answer');
for (let i = 0; i < 64; i++) {
  const o: any = Object.create(holder);
  o['padding' + i] = i;
  o.answer = i;
  o[key] = i + 100;
  values.push(o);
}
let sum = 0;
for (let pass = 0; pass < 4; pass++) {
  for (let i = 0; i < values.length; i++) sum += read(values[i], 'answer') + read(values[i], key);
}
console.log(sum);
const o = values[63];
console.log(read(o, 'answer'));
delete o.answer;
console.log(read(o, 'answer'));
holder.answer = 8;
console.log(read(o, 'answer'));
let calls = 0;
Object.defineProperty(o, 'answer', { configurable: true, get() { calls++; return this[key]; } });
console.log(read(o, 'answer'), read(o, 'answer'), calls);
delete o.answer;
Object.setPrototypeOf(o, { answer: 9 });
console.log(read(o, 'answer'));
Object.setPrototypeOf(o, new Proxy({ answer: 10 }, { get(t, k, receiver) { calls++; return Reflect.get(t, k, receiver); } }));
console.log(read(o, 'answer'), calls);
const large: any = {};
for (let i = 0; i < 64; i++) large['property' + i] = i;
console.log(read(large, 'property' + 63), read(large, 'property' + 64));
console.log(read(['a', 'bb'], 1), read('abc', 2));
console.log(read({ 2: 'numeric' }, 2));
console.log(read({ get answer() { calls++; throw new Error('read once'); } }, 'missing'));
try { read({ get answer() { calls++; throw new Error('read once'); } }, 'answer'); } catch (e) { console.log((e as Error).message, calls); }
