// `new Function` of runtime source deletes Object.keys: a direct call must then
// throw the TypeError of calling undefined, and `typeof` must see the delete,
// like node.
const o = { x: 1, y: 2 };
function keysOf(v: object): string[] {
    return Object.keys(v);
}
console.log(keysOf(o).join(","), Object.values(o).join(","), typeof Object.keys);
const body = process.argv[2] || "delete Object.keys;";
new Function(body)();
console.log(typeof Object.keys, typeof Object.values, Object.values(o).join(","));
try {
    console.log(keysOf(o));
} catch (e) {
    console.log("threw", e instanceof TypeError);
}
const created = Object.create({ inherited: 1 });
console.log(Object.getPrototypeOf(created).inherited, Object.entries(o).length);
