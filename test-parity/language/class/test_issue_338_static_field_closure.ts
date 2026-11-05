// Issue #338: closure-collector misses static field initializers with
// closure values. `Union.make` / `Union.unify` in Effect's SchemaAST.ts
// are static fields holding `() => ...` arrows; pre-fix the collector
// walked `c.fields` (instance) but not `c.static_fields`.

class Holder {
    static add = (a: number, b: number) => a + b;
    static greet = (name: string) => "Hello, " + name + "!";
    static multiply = (x: number, y: number) => x * y;
    instanceField = 10;
}

console.log(Holder.add(2, 3));
console.log(Holder.greet("world"));
console.log(Holder.multiply(4, 5));

// Also exercise nested closures inside static field inits — closure
// inside `Array.map` callback inside static field init.
class Builder {
    static buildList = (n: number) => [1, 2, 3].map((x) => x * n);
}
const list = Builder.buildList(10);
console.log(list[0], list[1], list[2]);
