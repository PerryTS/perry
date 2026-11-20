import { types } from "node:util";
console.log("async generator predicate", typeof (types as any).isAsyncGeneratorFunction);
const syncParent = Object.getPrototypeOf(function* () {}.prototype);
const asyncParent = Object.getPrototypeOf(async function* () {}.prototype);
const key = Symbol(process.argv[2] ?? "computed");
class Sync { *[key]() { yield 7; } }
class Async { async *[key]() { yield 8; } }
class StaticSync { static *[key]() { yield 9; } }
class StaticAsync { static async *[key]() { yield 10; } }
const syncObject = { *[key]() { yield 11; } };
const asyncObject = { async *[key]() { yield 12; } };
const receiverObject = { value: 13, async *[key]() { yield this.value; } };
class Empty { *[key]() {} }
class EmptyAsync { async *[key]() {} }
const emptyObject = { *[key]() {} };
const emptyAsyncObject = { async *[key]() {} };
for (const [name, fn, parent] of [
    ["empty class sync", Empty.prototype[key], syncParent],
    ["empty class async", EmptyAsync.prototype[key], asyncParent],
    ["empty object sync", emptyObject[key], syncParent],
    ["empty object async", emptyAsyncObject[key], asyncParent],
    ["class sync", Sync.prototype[key], syncParent],
    ["class async", Async.prototype[key], asyncParent],
    ["static sync", StaticSync[key], syncParent],
    ["static async", StaticAsync[key], asyncParent],
    ["object sync", syncObject[key], syncParent],
    ["object async", asyncObject[key], asyncParent],
    ["object receiver async", receiverObject[key], asyncParent],
] as any[]) {
    const descriptor = Object.getOwnPropertyDescriptor(fn, "prototype");
    console.log(name, Object.hasOwn(fn, "prototype"), typeof fn.prototype);
    console.log("function kind", types.isGeneratorFunction(fn), types.isAsyncFunction(fn));
    if (descriptor) console.log(descriptor.writable, descriptor.enumerable, descriptor.configurable,
        Object.getPrototypeOf(fn.prototype) === parent,
        Object.getPrototypeOf(fn()) === fn.prototype);
    console.log((await fn.call(receiverObject).next()).value);
}
const instance = new Async();
console.log("instance identity", instance[key] === Async.prototype[key]);
const replacement = async function* () { yield 99; };
Object.setPrototypeOf(instance, { [key]: replacement });
console.log("actual prototype", instance[key] === replacement, (await instance[key]().next()).value);
