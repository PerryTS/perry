// A member parent with the same spelling as the child is still a real parent.
export class Error extends globalThis.Error {
    tag = "own";
    constructor(props: { message: string }) { super(props.message); }
}
export class TypeError extends globalThis.TypeError {
    code = 42;
    constructor(message: string) { super(message); }
}
export class Map extends globalThis.Map {
    tag = "map";
    constructor() { super(); if (this instanceof globalThis.Map) this.set("k", 7); }
}
const error = new Error({ message: "rg" });
console.log(error.message, error.tag, error instanceof globalThis.Error);
console.log(Object.getPrototypeOf(Error) === globalThis.Error);
const typeError = new TypeError("type");
console.log(typeError.message, typeError.code, typeError instanceof globalThis.TypeError, typeError instanceof globalThis.Error);
console.log(Object.getPrototypeOf(TypeError) === globalThis.TypeError);
const map = new Map();
console.log(map instanceof globalThis.Map ? map.get("k") : undefined, map.size, map.tag, map instanceof globalThis.Map);
console.log(Object.getPrototypeOf(Map) === globalThis.Map);
const ExprError = class Error extends globalThis.Error {};
const expr = new ExprError("expression");
console.log(expr.message, expr instanceof globalThis.Error, Object.getPrototypeOf(ExprError) === globalThis.Error);
