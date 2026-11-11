// #1175: Object.create(null) must be observable as a null-prototype object,
// independently from querystring.parse.
const dict: any = Object.create(null);
dict.a = "1";
dict["__proto__"] = "own-proto";
dict.constructor = "own-constructor";

console.log("proto is null:", Object.getPrototypeOf(dict) === null);
console.log("keys:", Object.keys(dict).sort().join(","));
console.log("__proto__:", dict["__proto__"]);
console.log("constructor:", dict.constructor);
console.log("json:", JSON.stringify(dict));

const assigned = Object.assign({}, dict);
console.log("assigned proto is null:", Object.getPrototypeOf(assigned) === null);
console.log("assigned a:", assigned.a);
