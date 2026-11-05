// Refs v0.5.751: Object.getPrototypeOf for class refs walks the
// CLASS_REGISTRY parent_class_id chain. Pre-fix the codegen wrapper
// returned the operand unchanged, so `cur = Object.getPrototypeOf(cur)`
// in a while-loop never terminated — drizzle's `is(value, type)` chain
// hangs on subclass walking.
class Base {
    static kind = "Base";
}
class Mid extends Base {
    static kind = "Mid";
}
class Leaf extends Mid {
    static kind = "Leaf";
}

const p1 = Object.getPrototypeOf(Leaf);
console.log("typeof p1:", typeof p1);
console.log("p1 === Mid:", p1 === Mid);

const p2 = Object.getPrototypeOf(Mid);
console.log("typeof p2:", typeof p2);
console.log("p2 === Base:", p2 === Base);

// Walk loop using `cur[kind]` as termination — matches drizzle's
// is(value, type) chain pattern. After Base, getPrototypeOf returns
// Function.prototype (Node) or null (Perry), but cur.kind is undefined
// either way, so the loop terminates identically.
let cur: any = Leaf;
const seen: string[] = [];
while (cur && cur.kind) {
    seen.push(cur.kind);
    cur = Object.getPrototypeOf(cur);
}
console.log("walk:", JSON.stringify(seen));
