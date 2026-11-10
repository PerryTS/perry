const obj: any = { visible: 1 };
Object.defineProperty(obj, "hidden", { value: 2, enumerable: false });
console.dir(obj, { showHidden: true });
