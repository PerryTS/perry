const a = import("./m.ts");
const b = import("./m.ts");
Promise.all([a, b]).then(([x, y]) => console.log("same", x === y, x.ready));
console.log("n");
