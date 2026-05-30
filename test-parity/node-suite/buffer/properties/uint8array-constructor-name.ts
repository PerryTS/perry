const direct = new Uint8Array([1, 2]);
const byLength = new Uint8Array(2);

console.log("direct ctor:", direct.constructor.name);
console.log("length ctor:", byLength.constructor.name);
