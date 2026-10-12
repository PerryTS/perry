import { AsyncLocalStorage } from 'node:async_hooks';
const als = new AsyncLocalStorage<{value: number}>();
let snapshot: any;
als.run({value: 37}, () => { snapshot = AsyncLocalStorage.snapshot(); });
for (let i = 0; i < 40000; i++) { const a = { i, nested: [i, i + 1] }; if (i === 39999) console.log(a.i); }
als.run({value: 11}, () => {
    console.log(snapshot(() => als.getStore()!.value));
    console.log(als.getStore()!.value);
});
console.log(snapshot(() => als.getStore()!.value));
console.log(als.getStore());
