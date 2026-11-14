const names=["WeakRef","FinalizationRegistry","WeakSet","AggregateError","Float64Array","BigInt64Array","Int16Array","URIError","EvalError","SharedArrayBuffer"];
const C=names.map(n=>globalThis[n]);
for(let i=0;i<C.length;i++){const p=C[i].prototype; console.log(names[i], typeof p, p.constructor===C[i], Object.getPrototypeOf(p)===(i>=4&&i<=6?Object.getPrototypeOf(Int8Array.prototype):(i==3||i>=7&&i<=8?Error.prototype:Object.prototype)))}
console.log(WeakSet.prototype===C[2].prototype, Object.getPrototypeOf(new WeakSet())===C[2].prototype, Object.getPrototypeOf(new Float64Array(1))===C[4].prototype);
