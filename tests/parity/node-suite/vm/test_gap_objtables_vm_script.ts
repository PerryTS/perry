import { createScript, createContext } from 'node:vm';
const script = createScript('x += 1; x', {filename: 'owned-script.ts'});
const context = createContext({x: 36});
for (let i = 0; i < 40000; i++) { const a = { i, nested: [i, i + 1] }; if (i === 39999) console.log(a.i); }
console.log(script.runInContext(context));
console.log(script.runInContext(context));
console.log(Object.getOwnPropertySymbols(script).length);
