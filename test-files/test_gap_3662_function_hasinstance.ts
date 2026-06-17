// #3662 — `Function.prototype[Symbol.hasInstance]` must exist and implement
// `OrdinaryHasInstance`. Pre-fix it was `undefined`: `typeof` reported
// "undefined" and any reflective use threw. Unlike the `instanceof` operator
// (which throws a TypeError on a non-callable RHS), `OrdinaryHasInstance`
// returns `false` for a non-callable `this`. We print booleans / error *types*
// so the output is byte-identical to Node.

function r(fn: () => unknown): string {
    try {
        return String(fn());
    } catch (e: any) {
        return "throw:" + e.constructor.name;
    }
}

const hi = Function.prototype[Symbol.hasInstance];
console.log("typeof:", typeof hi);

// Non-callable `this` -> false (not a TypeError).
console.log("undefined:", r(() => hi.call(undefined, {})));
console.log("number:", r(() => hi.call(5, {})));
console.log("plainobj:", r(() => hi.call({}, [])));

// Non-object value with a callable `this` -> false.
console.log("value-prim:", r(() => hi.call(Array, 5)));

// User classes resolve through the normal prototype-chain walk.
class Animal {}
class Dog extends Animal {}
const d = new Dog();
console.log("Dog d:", r(() => hi.call(Dog, d)));
console.log("Animal d:", r(() => hi.call(Animal, d)));
console.log("Dog {}:", r(() => hi.call(Dog, {})));

// Built-in reference constructors used as runtime values.
console.log("Map m:", r(() => hi.call(Map, new Map())));
console.log("Set s:", r(() => hi.call(Set, new Set())));
console.log("RegExp re:", r(() => hi.call(RegExp, /x/)));
console.log("Set m:", r(() => hi.call(Set, new Map())));

// The `instanceof` operator itself is unchanged.
console.log("operator:", d instanceof Dog, d instanceof Animal, ({}) instanceof Dog, [] instanceof Array);
