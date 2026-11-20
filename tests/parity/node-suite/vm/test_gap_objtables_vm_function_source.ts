import { compileFunction } from 'node:vm';
const f = compileFunction('return x + 1;', ['x']);
for (let i = 0; i < 40000; i++) { const a = { i, nested: [i, i + 1] }; if (i === 39999) console.log(a.i); }
console.log(f(36));
console.log(f.toString().includes('return x + 1;'));
console.log(f.length);
console.log(Object.getOwnPropertySymbols(f).length);
