// Refs v0.5.746: `instance.constructor` returns the class ref.
// Pre-fix returned undefined because the keys_array lookup never
// finds "constructor" (the class isn't stored as a field on the
// instance) and the chain returned undefined. Drizzle's `is(value, type)`
// uses `value.constructor[entityKind]` to identify class types.
const KIND = Symbol.for("test:kind");

class SQL {
    static [KIND] = "SQL";
}

class Aliased {
    static [KIND] = "Aliased";
}

const a = new Aliased();
const s = new SQL();

console.log("typeof a.constructor:", typeof a.constructor);
console.log("a.constructor === Aliased:", a.constructor === Aliased);
console.log("a.constructor[KIND]:", (a.constructor as any)[KIND]);
console.log("s.constructor[KIND]:", (s.constructor as any)[KIND]);

function is(value: any, type: any): boolean {
    if (!value) return false;
    if (value instanceof type) return true;
    return value.constructor?.[KIND] === type[KIND];
}
console.log("is(a, Aliased):", is(a, Aliased));
console.log("is(a, SQL):", is(a, SQL));
console.log("is(s, SQL):", is(s, SQL));
console.log("is(s, Aliased):", is(s, Aliased));
