// #10497: %Function.prototype% is a per-realm intrinsic. `globalThis.Function`
// is a writable, configurable global binding, so reassigning it must not change
// which object the runtime treats as Function.prototype -- not for the
// prototype of existing functions, not for method dispatch through it, and not
// for Object.hasOwn on it (whose answer for the inherited Object.prototype
// methods depends on recognising the intrinsic).
const FP = Function.prototype;
function named() { return 'named-body'; }
(FP as any).helper10497 = function (this: any) { return 'helper:' + this.name; };

const RealFunction = globalThis.Function;
(globalThis as any).Function = function FakeFunction() {};
(globalThis as any).Function.prototype = { fake: true };

let log = '';
for (let i = 0; i < 300; i++) {
    const r = (named as any).helper10497();
    if (i === 0 || i === 299) log += r + ';';
}
console.log('proto identity', Object.getPrototypeOf(named) === FP);
console.log('dispatch', log);
console.log('hasOwn hasOwnProperty', Object.hasOwn(FP, 'hasOwnProperty'));
console.log('hasOwn helper', Object.hasOwn(FP, 'helper10497'));
console.log('hasOwn call', Object.hasOwn(FP, 'call'));

(globalThis as any).Function = RealFunction;
console.log('restored', globalThis.Function === RealFunction, Function.prototype === FP);
