import { createContext, isContext, runInContext } from 'node:vm';
const sandbox = { value: { x: 37 } };
const context = createContext(sandbox);
for (let i = 0; i < 40000; i++) { const a = { i, nested: [i, i + 1] }; if (i === 39999) console.log(a.i); }
console.log(isContext(context));
console.log(context === sandbox);
console.log(createContext(sandbox) === context);
console.log(runInContext('value.x', context));
console.log(isContext({}));
console.log(Object.keys(sandbox).join(','));
