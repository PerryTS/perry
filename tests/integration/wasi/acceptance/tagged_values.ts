class Base { m() { return 1; } }
const B: any = Base;
console.log(typeof B["prototype"]);
console.log(Object.getPrototypeOf(new Base()) === Base.prototype);
const o = {};
Object.defineProperty(o, "x", { value: 42, enumerable: true });
console.log(Reflect.get(o, "x"), Object.keys(o).join(","));
