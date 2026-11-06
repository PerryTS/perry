// Object.assign / object-spread with an exotic source (Map/Set/Date/RegExp)
// must not crash: such objects expose no own enumerable string keys, so they
// contribute nothing (CopyDataProperties). Regression test for #6070, where a
// Map/Set source deref'd its header bytes as an ObjectHeader.keys_array — the
// bogus keys pointer was then walked as an array (a memory-layout-dependent
// SIGBUS, made deterministic here by sourcing through `any`-typed bindings).
const m: any = new Map([["a", 1]]);
const s: any = new Set([1, 2, 3]);
console.log(JSON.stringify(Object.assign({ z: 0 }, m)));
console.log(JSON.stringify(Object.assign({ z: 0 }, s)));
console.log(JSON.stringify({ z: 0, ...m }));
console.log(JSON.stringify(Object.assign({}, new Date(0))));
console.log(JSON.stringify(Object.assign({}, /x/g)));
// The any-typed loop is what made the crash reliably reproduce.
const cases: any[] = [new Map([["a", 1]]), new Set([1]), new Date(0), /y/];
let out = "";
for (const src of cases) out += JSON.stringify(Object.assign({ z: 0 }, src)) + ";";
console.log(out);
// Genuine object and array sources still copy their own enumerable props.
console.log(JSON.stringify(Object.assign({ a: 1 }, { b: 2 }, [10, 20])));
