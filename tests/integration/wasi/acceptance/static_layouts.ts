import { collect } from "perry/gc";

// Runtime-declared shapes and image atoms must survive moving collections.
// Calling through the prototype also exercises the Rust builtin entry ABI,
// independently of direct compiler lowering of array methods.
const data = JSON.parse('{"alpha":1,"beta":"two"}');
const url = new URL("https://example.com/path?q=value");
const failure = new Error("retained");
for (let i = 0; i < 5; i++) collect();
console.log(Object.keys(data).join(","), data.beta);
console.log(url.hostname, url.searchParams.get("q"));
console.log(failure.message);
const items = [1, 2, 3];
const map: any = Array.prototype.map;
const filter: any = Array.prototype.filter;
console.log(map.call(items, (x: number) => x * 2).join(","));
console.log(filter.call(items, (x: number) => x > 1).join(","));
console.log(items.toReversed().join(","), items.with(1, 9).join(","));
