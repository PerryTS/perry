import * as path from "node:path";
// Each declined receiver exercises its ordinary generic operation, compared to Node.
const literal = { x: 1, y: 2 };
console.log("literal", literal.constructor === Object, Object.getPrototypeOf(literal) === Object.prototype);
const empty = {};
console.log("empty", empty.constructor === Object, Object.getPrototypeOf(empty) === Object.prototype);
class Empty {}
const instance = new Empty();
console.log("declared", instance.constructor === Empty, Object.getPrototypeOf(instance) === Empty.prototype);
const custom: any = { constructor: function Other() {}, inherited: 8 };
const linked: any = Object.create(custom);
linked.x = 3;
linked.y = 4;
console.log("linked", linked.constructor === custom.constructor, Object.getPrototypeOf(linked) === custom);
console.log("assign", JSON.stringify(Object.assign({}, linked)));
const { x, ...rest } = linked;
console.log("rest", x, JSON.stringify(rest));
let keys = "";
for (const key in linked) keys += key + ",";
console.log("forin", keys);
delete linked.y;
console.log("delete", "y" in linked, linked.inherited);
let observed = 0;
const setterProto: any = { set value(n: number) { observed = n; }, get value() { return observed + 1; } };
const receiver: any = Object.create(setterProto);
receiver.value = 12;
console.log("setter", observed, receiver.value, Object.hasOwn(receiver, "value"));
const arraylike: any = Object.create(null);
arraylike[0] = "first";
arraylike.length = 1;
console.log("borrow", Array.prototype.pop.call(arraylike), arraylike.length);
console.log("null", Object.getPrototypeOf(arraylike) === null, arraylike.constructor === undefined, typeof arraylike.toString);
const inheritedArrayLike: any = Object.create(custom);
inheritedArrayLike[0] = "second";
inheritedArrayLike.length = 1;
console.log("borrow-linked", Array.prototype.pop.call(inheritedArrayLike), inheritedArrayLike.length);
class ReturningBase { constructor() { return Object.create(setterProto); } }
class DefineOnReturned extends ReturningBase { value = 33; }
const returned: any = new DefineOnReturned();
console.log("define-returned", observed, returned.value, Object.hasOwn(returned, "value"), Object.getPrototypeOf(returned) === setterProto);

class WithPrivate { #secret = 9; x = 5; value() { return this.#secret; } }
const privateInstance = new WithPrivate();
console.log("private", privateInstance.value(), JSON.stringify(Object.assign({}, privateInstance)));
const jsonProto: any = { toJSON() { return { transformed: 19 }; } };
const jsonLinked = Object.create(jsonProto);
console.log("json", JSON.stringify(jsonLinked));
const thenProto: any = { then(resolve: any) { resolve(23); } };
Promise.resolve(Object.create(thenProto)).then(value => console.log("then", value));
const saved = Object.getOwnPropertyDescriptor(Object.prototype, "constructor")!;
Object.defineProperty(Object.prototype, "constructor", { value: 41, writable: true, configurable: true });
console.log("edited", literal.constructor === 41, empty.constructor === 41);
delete (Object.prototype as any).constructor;
console.log("deleted", literal.constructor === undefined, empty.constructor === undefined);
Object.defineProperty(Object.prototype, "constructor", saved);

// Literal method fallback must retain the ordinary value and bind semantics.
const methods: any = { n: 4, m() { return this.n + 1; } };
const originalMethod = methods.m;
console.log("literal-method", methods.m(), originalMethod.call({ n: 8 }), methods.m === originalMethod);
methods.m = methods.m.bind(methods);
console.log("literal-bound", methods.m(), methods.m.call({ n: 99 }));
const inheritedMethod: any = Object.create(methods);
inheritedMethod.n = 12;
console.log("inherited-bound", inheritedMethod.m(), inheritedMethod.toString === Object.prototype.toString);
class MethodClass { n = 6; m() { return this.n + 2; } }
const classMethod: any = new MethodClass();
const unboundClassMethod = classMethod.m;
console.log("class-method", classMethod.m(), unboundClassMethod.call({ n: 10 }), classMethod.m === MethodClass.prototype.m);

const nativeJoin = path.join.bind(path);
console.log("native-bound", nativeJoin("a", "b"), path.join("a", "b"));
