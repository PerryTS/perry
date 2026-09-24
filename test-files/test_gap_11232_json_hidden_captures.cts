// #11232: serializer options must not expose a class's compiler capture slots.
const M = require("./_helpers/json_hidden_captures_11232.cjs");
const value = new M.Plain(1);
console.log(JSON.stringify(value));
const seen: string[] = [];
console.log(JSON.stringify(value, (key, item) => { seen.push(key); return item; }));
console.log(seen.join(","));
console.log(JSON.stringify(value, null, 1).replace(/\n/g, ""));
console.log(JSON.stringify(value, (key, item) => item, 2).replace(/\n/g, ""));
console.log(JSON.stringify(value, ["t", "a", "__perry_cap_user"], 1).replace(/\n/g, ""));
console.log(Object.keys(value).join(","), Reflect.ownKeys(value).length);
// The hidden value must not be traversed, even when it contains a cycle.
const cycle = new M.Cyclic();
console.log(cycle.captured().self === cycle.captured());
console.log(JSON.stringify(cycle, (key, item) => item));
console.log(JSON.stringify(cycle, null, 1).replace(/\n/g, ""));
// Plain objects may use even exact reserved-looking keys as public properties.
const ordinary = JSON.parse('{"__perry_cap_7":7,"__perry_cap_45m0000331fa678":8,"__perry_cap_user":9}');
console.log(JSON.stringify(ordinary, (key, item) => item));
console.log(JSON.stringify(ordinary, null, 1).replace(/\n/g, ""));
console.log(JSON.stringify(ordinary, ["__perry_cap_7", "__perry_cap_45m0000331fa678", "__perry_cap_user"]));
