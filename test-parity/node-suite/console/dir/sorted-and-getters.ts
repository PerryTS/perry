const obj: any = { b: 2, a: 1 };
Object.defineProperty(obj, "g", { get() { return 3; }, enumerable: true });
console.dir(obj, { sorted: true });
console.dir(obj, { getters: true });
